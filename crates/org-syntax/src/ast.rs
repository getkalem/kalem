//! Typed wrappers over the untyped syntax tree.
//!
//! Every org-element type has a wrapper, such as [`Headline`] or [`Link`],
//! with accessors for the properties Emacs computes (`:todo-keyword`,
//! `:path`, `:repeater-unit`, ...). Values are derived from the tree on
//! demand; the tree itself stays the single source of truth.
//!
//! Generic helpers work on any node: [`contents_range`], [`post_blank`],
//! [`post_affiliated`] and [`affiliated_keywords`] reproduce org-element's
//! `:contents-begin`/`:contents-end`, `:post-blank` and `:post-affiliated`.

use rowan::{NodeOrToken, TextRange, TextSize};

use crate::{ParseContext, SyntaxElement, SyntaxKind, SyntaxKind::*, SyntaxNode, SyntaxToken, re};

/// A typed view of a syntax node.
pub trait AstNode: Sized {
    /// Returns `true` if nodes of `kind` can be viewed as `Self`.
    fn can_cast(kind: SyntaxKind) -> bool;
    /// Views `node` as `Self` if its kind matches.
    fn cast(node: SyntaxNode) -> Option<Self>;
    /// The underlying node.
    fn syntax(&self) -> &SyntaxNode;
    /// The node's text range (`:begin` and `:end`).
    fn range(&self) -> TextRange {
        self.syntax().text_range()
    }
}

macro_rules! ast_nodes {
    ($($(#[$m:meta])* $name:ident = $kind:ident;)*) => {
        $(
            $(#[$m])*
            #[derive(Debug, Clone, PartialEq, Eq, Hash)]
            pub struct $name(SyntaxNode);

            impl AstNode for $name {
                fn can_cast(kind: SyntaxKind) -> bool {
                    kind == $kind
                }
                fn cast(node: SyntaxNode) -> Option<Self> {
                    Self::can_cast(node.kind()).then(|| $name(node))
                }
                fn syntax(&self) -> &SyntaxNode {
                    &self.0
                }
            }
        )*
    };
}

ast_nodes! {
    /// The whole document (`org-data`).
    Document = DOCUMENT;
    /// A section: the text between a headline and its first child.
    Section = SECTION;
    /// A headline and its subtree.
    Headline = HEADLINE;
    /// An inline task.
    Inlinetask = INLINETASK;
    /// A planning line (SCHEDULED, DEADLINE, CLOSED).
    Planning = PLANNING;
    /// A `:PROPERTIES:` drawer.
    PropertyDrawer = PROPERTY_DRAWER;
    /// A line in a property drawer.
    NodeProperty = NODE_PROPERTY;
    /// A generic drawer such as `:LOGBOOK:`.
    Drawer = DRAWER;
    /// A plain list.
    PlainList = PLAIN_LIST;
    /// A list item.
    Item = ITEM;
    /// A table (Org or table.el).
    Table = TABLE;
    /// A table row.
    TableRow = TABLE_ROW;
    /// A table cell.
    TableCell = TABLE_CELL;
    /// `#+BEGIN_CENTER`.
    CenterBlock = CENTER_BLOCK;
    /// `#+BEGIN_QUOTE`.
    QuoteBlock = QUOTE_BLOCK;
    /// Any other `#+BEGIN_NAME` block.
    SpecialBlock = SPECIAL_BLOCK;
    /// `#+BEGIN: name args`.
    DynamicBlock = DYNAMIC_BLOCK;
    /// `[fn:label] definition`.
    FootnoteDefinition = FOOTNOTE_DEFINITION;
    /// `#+CALL:`.
    BabelCall = BABEL_CALL;
    /// A clock line.
    Clock = CLOCK;
    /// Comment lines starting with `#`.
    Comment = COMMENT;
    /// `#+BEGIN_COMMENT`.
    CommentBlock = COMMENT_BLOCK;
    /// `%%(...)` diary sexp.
    DiarySexp = DIARY_SEXP;
    /// `#+BEGIN_EXAMPLE`.
    ExampleBlock = EXAMPLE_BLOCK;
    /// `#+BEGIN_EXPORT backend`.
    ExportBlock = EXPORT_BLOCK;
    /// Lines starting with `: `.
    FixedWidth = FIXED_WIDTH;
    /// `-----`.
    HorizontalRule = HORIZONTAL_RULE;
    /// `#+KEY: value`.
    Keyword = KEYWORD;
    /// `\begin{env}...\end{env}`.
    LatexEnvironment = LATEX_ENVIRONMENT;
    /// A paragraph.
    Paragraph = PARAGRAPH;
    /// `#+BEGIN_SRC lang`.
    SrcBlock = SRC_BLOCK;
    /// `#+BEGIN_VERSE`.
    VerseBlock = VERSE_BLOCK;
    /// `*bold*`.
    Bold = BOLD;
    /// `/italic/`.
    Italic = ITALIC;
    /// `_underline_`.
    Underline = UNDERLINE;
    /// `+strike-through+`.
    StrikeThrough = STRIKE_THROUGH;
    /// `~code~`.
    Code = CODE;
    /// `=verbatim=`.
    Verbatim = VERBATIM;
    /// `[cite:@key]`.
    Citation = CITATION;
    /// One reference inside a citation.
    CitationReference = CITATION_REFERENCE;
    /// `\alpha`.
    Entity = ENTITY;
    /// `@@backend:value@@`.
    ExportSnippet = EXPORT_SNIPPET;
    /// `[fn:label]` or `[fn:label:definition]`.
    FootnoteReference = FOOTNOTE_REFERENCE;
    /// `call_name(args)`.
    InlineBabelCall = INLINE_BABEL_CALL;
    /// `src_lang{code}`.
    InlineSrcBlock = INLINE_SRC_BLOCK;
    /// `$x$`, `\(x\)`, `\[x\]`, `\macro{}`.
    LatexFragment = LATEX_FRAGMENT;
    /// `\\` at the end of a line.
    LineBreak = LINE_BREAK;
    /// A link of any format.
    Link = LINK;
    /// `{{{name(args)}}}`.
    Macro = MACRO;
    /// `<<<target>>>`.
    RadioTarget = RADIO_TARGET;
    /// `[1/3]` or `[33%]`.
    StatisticsCookie = STATISTICS_COOKIE;
    /// `x_1`, `x_{12}`.
    Subscript = SUBSCRIPT;
    /// `x^2`, `x^{12}`.
    Superscript = SUPERSCRIPT;
    /// `<<target>>`.
    Target = TARGET;
    /// A timestamp.
    Timestamp = TIMESTAMP;
    /// An affiliated keyword such as `#+NAME:` attached to an element.
    AffiliatedKeyword = AFFILIATED_KEYWORD;
}

// ------------------------------------------------------------------------
// Generic helpers.

fn tokens(node: &SyntaxNode) -> impl Iterator<Item = SyntaxToken> + '_ {
    node.children_with_tokens()
        .filter_map(NodeOrToken::into_token)
}

fn token(node: &SyntaxNode, kind: SyntaxKind) -> Option<SyntaxToken> {
    tokens(node).find(|t| t.kind() == kind)
}

fn child(node: &SyntaxNode, kind: SyntaxKind) -> Option<SyntaxNode> {
    node.children().find(|c| c.kind() == kind)
}

fn markers(node: &SyntaxNode) -> Vec<SyntaxToken> {
    tokens(node).filter(|t| t.kind() == MARKER).collect()
}

fn text(node: &SyntaxNode) -> String {
    node.text().to_string()
}

/// Text of `node` from `start` (absolute) to its end of line.
fn rest_of_line(node: &SyntaxNode, start: TextSize) -> String {
    let full = text(node);
    let off = usize::from(start - node.text_range().start());
    let rest = &full[off.min(full.len())..];
    let end = rest.find(['\n', '\r']).unwrap_or(rest.len());
    rest[..end].to_string()
}

/// Converts `\r\n` to `\n`, as Emacs does when it reads a DOS file.
fn lf(s: String) -> String {
    if s.contains('\r') {
        s.replace("\r\n", "\n")
    } else {
        s
    }
}

/// `org-trim`.
pub(crate) fn org_trim(s: &str) -> &str {
    s.trim_matches(|c| matches!(c, ' ' | '\t' | '\n' | '\r'))
}

/// `org-string-nw-p`.
fn nw(s: &str) -> Option<String> {
    s.chars()
        .any(|c| !matches!(c, ' ' | '\t' | '\n' | '\r'))
        .then(|| s.to_string())
}

fn is_blank_token(e: &SyntaxElement) -> bool {
    e.kind() == BLANK_LINE
}

/// Elements that make up an element's contents.
fn content_elements(node: &SyntaxNode) -> impl Iterator<Item = SyntaxNode> + '_ {
    node.children().filter(|c| c.kind().is_element())
}

/// `:post-affiliated`: the start of the element after its affiliated
/// keywords.
pub fn post_affiliated(node: &SyntaxNode) -> TextSize {
    node.children()
        .filter(|c| c.kind() == AFFILIATED_KEYWORD)
        .map(|c| c.text_range().end())
        .max()
        .unwrap_or(node.text_range().start())
}

/// The affiliated keywords of an element.
pub fn affiliated_keywords(node: &SyntaxNode) -> impl Iterator<Item = AffiliatedKeyword> + '_ {
    node.children().filter_map(AffiliatedKeyword::cast)
}

fn count_lines(s: &str) -> usize {
    if s.is_empty() {
        return 0;
    }
    let n = s.bytes().filter(|&b| b == b'\n').count();
    if s.ends_with('\n') { n } else { n + 1 }
}

/// `:post-blank`: trailing blank lines after an element, or trailing
/// spaces and tabs after an object.
pub fn post_blank(node: &SyntaxNode) -> usize {
    let kind = node.kind();
    if kind.is_object() {
        if matches!(kind, LINE_BREAK | TABLE_CELL | CITATION_REFERENCE) {
            return 0;
        }
        return match node.last_child_or_token() {
            Some(NodeOrToken::Token(t)) if t.kind() == WHITESPACE => t.text().chars().count(),
            _ => 0,
        };
    }
    match kind {
        DOCUMENT | SECTION | NODE_PROPERTY | TABLE_ROW => return 0,
        HEADLINE | INLINETASK if content_elements(node).next().is_some() && kind == HEADLINE => {
            return 0;
        }
        _ => {}
    }
    let trailing: String = {
        let all: Vec<SyntaxElement> = node.children_with_tokens().collect();
        let k = all.iter().rev().take_while(|e| is_blank_token(e)).count();
        all[all.len() - k..].iter().map(|e| e.to_string()).collect()
    };
    let blank = count_lines(&trailing);
    match kind {
        ITEM => {
            // `(count-lines (or contents-end begin) end)`.
            match contents_range(node) {
                Some(_) => blank,
                None => {
                    let t = text(node);
                    count_lines(&t)
                }
            }
        }
        FIXED_WIDTH => {
            // Counted from the end of the last line, before its newline.
            let t = text(node);
            let body = &t[..t.len() - trailing.len()];
            if body.ends_with('\n') {
                blank + 1
            } else {
                blank
            }
        }
        BABEL_CALL => {
            // Counted from the line after the first line of the call.
            let pa = usize::from(post_affiliated(node) - node.text_range().start());
            let t = text(node);
            let first_line_end = t[pa..].find('\n').map_or(t.len(), |i| pa + i + 1);
            count_lines(&t[first_line_end..])
        }
        PARAGRAPH => {
            // A paragraph that starts on an empty line: Emacs counts that
            // line both as contents and as post-blank.
            match contents_range(node) {
                Some(c)
                    if c.len() == TextSize::from(1) && {
                        let base = node.text_range().start();
                        node.text()
                            .slice(TextRange::new(c.start() - base, c.end() - base))
                            == "\n"
                    } =>
                {
                    blank + 1
                }
                _ => blank,
            }
        }
        _ => blank,
    }
}

fn range_between(a: TextSize, b: TextSize) -> Option<TextRange> {
    (a <= b).then(|| TextRange::new(a, b))
}

/// `:contents-begin` and `:contents-end`, or `None` when the element or
/// object has no contents.
pub fn contents_range(node: &SyntaxNode) -> Option<TextRange> {
    let kind = node.kind();
    match kind {
        INLINETASK => {
            // Contents run up to the END line, and may be empty.
            let end_line = child(node, BLOCK_END)?;
            let end = end_line.text_range().start();
            let start = content_elements(node)
                .next()
                .map_or(end, |c| c.text_range().start());
            range_between(start, end)
        }
        DOCUMENT | SECTION | HEADLINE | PLAIN_LIST | ITEM | PROPERTY_DRAWER | DRAWER
        | CENTER_BLOCK | QUOTE_BLOCK | SPECIAL_BLOCK | DYNAMIC_BLOCK | FOOTNOTE_DEFINITION
        | TABLE => {
            let first = content_elements(node).next()?;
            let last = content_elements(node).last()?;
            range_between(first.text_range().start(), last.text_range().end())
        }
        PARAGRAPH => {
            let start = post_affiliated(node);
            let end = node
                .children_with_tokens()
                .collect::<Vec<_>>()
                .iter()
                .rev()
                .take_while(|e| is_blank_token(e))
                .last()
                .map_or(node.text_range().end(), |e| e.text_range().start());
            range_between(start, end)
        }
        VERSE_BLOCK => {
            let b = child(node, BLOCK_BEGIN)?;
            let e = child(node, BLOCK_END)?;
            range_between(b.text_range().end(), e.text_range().start())
        }
        TABLE_ROW => {
            let bar = token(node, MARKER)?;
            match node.children().filter(|c| c.kind() == TABLE_CELL).last() {
                Some(last) => range_between(bar.text_range().end(), last.text_range().end()),
                None => range_between(bar.text_range().end(), bar.text_range().end()),
            }
        }
        TABLE_CELL => {
            let items: Vec<SyntaxElement> = node
                .children_with_tokens()
                .filter(|e| !matches!(e.kind(), WHITESPACE | MARKER))
                .collect();
            match (items.first(), items.last()) {
                (Some(f), Some(l)) => range_between(f.text_range().start(), l.text_range().end()),
                _ => {
                    let at = token(node, MARKER)
                        .map_or(node.text_range().end(), |m| m.text_range().start());
                    range_between(at, at)
                }
            }
        }
        BOLD | ITALIC | UNDERLINE | STRIKE_THROUGH | RADIO_TARGET => {
            let m = markers(node);
            range_between(
                m.first()?.text_range().end(),
                m.last()?.text_range().start(),
            )
        }
        SUBSCRIPT | SUPERSCRIPT => {
            let m = markers(node);
            if m.len() >= 3 {
                range_between(m[1].text_range().end(), m[2].text_range().start())
            } else {
                let end = node.text_range().end() - TextSize::from(post_blank(node) as u32);
                range_between(m.first()?.text_range().end(), end)
            }
        }
        FOOTNOTE_REFERENCE => {
            let m = markers(node);
            (m.len() == 3)
                .then(|| range_between(m[1].text_range().end(), m[2].text_range().start()))?
        }
        LINK => {
            if token(node, CODE_TEXT).is_none() {
                // Radio link.
                let end = node.text_range().end() - TextSize::from(post_blank(node) as u32);
                return range_between(node.text_range().start(), end);
            }
            let m = markers(node);
            (m.len() == 3)
                .then(|| range_between(m[1].text_range().end(), m[2].text_range().start()))?
        }
        CITATION => {
            let refs: Vec<SyntaxNode> = node
                .children()
                .filter(|c| c.kind() == CITATION_REFERENCE)
                .collect();
            range_between(
                refs.first()?.text_range().start(),
                refs.last()?.text_range().end(),
            )
        }
        _ => None,
    }
}

// ------------------------------------------------------------------------
// Headlines.

/// The TODO state of a headline.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TodoType {
    /// A not-done keyword such as TODO.
    Todo,
    /// A done keyword such as DONE.
    Done,
}

fn heading_level(node: &SyntaxNode, ctx: &ParseContext) -> usize {
    let n = token(node, STARS).map_or(0, |t| t.text().len());
    if ctx.odd_levels_only && n > 0 {
        1 + n / 2
    } else {
        n
    }
}

macro_rules! heading_accessors {
    ($t:ty) => {
        impl $t {
            /// The number of stars.
            pub fn true_level(&self) -> usize {
                token(&self.0, STARS).map_or(0, |t| t.text().len())
            }
            /// `:level`, taking `#+STARTUP: odd` into account.
            pub fn level(&self, ctx: &ParseContext) -> usize {
                heading_level(&self.0, ctx)
            }
            /// `:todo-keyword`.
            pub fn todo_keyword(&self) -> Option<SyntaxToken> {
                token(&self.0, TODO_KEYWORD)
            }
            /// `:todo-type`.
            pub fn todo_type(&self, ctx: &ParseContext) -> Option<TodoType> {
                let k = self.todo_keyword()?;
                Some(if ctx.is_done_keyword(k.text()) {
                    TodoType::Done
                } else {
                    TodoType::Todo
                })
            }
            /// `:priority`: the priority character.
            pub fn priority(&self) -> Option<char> {
                let t = token(&self.0, PRIORITY)?;
                t.text()[2..].chars().next()
            }
            /// `:commentedp`.
            pub fn is_commented(&self) -> bool {
                token(&self.0, COMMENT_KEYWORD).is_some()
            }
            /// `:tags`: the headline's own tags.
            pub fn tags(&self) -> Vec<String> {
                // `org-split-string` on ":": only the leading and trailing
                // separators are dropped, so `::a:` has an empty first tag.
                token(&self.0, TAGS)
                    .map(|t| {
                        let inner = t.text().strip_prefix(':').unwrap_or(t.text());
                        let inner = inner.strip_suffix(':').unwrap_or(inner);
                        if inner.is_empty() {
                            Vec::new()
                        } else {
                            inner.split(':').map(str::to_string).collect()
                        }
                    })
                    .unwrap_or_default()
            }
            /// `:archivedp`.
            pub fn is_archived(&self) -> bool {
                self.tags().iter().any(|t| t == "ARCHIVE")
            }
            /// The parsed title.
            pub fn title(&self) -> Option<SyntaxNode> {
                child(&self.0, HEADLINE_TITLE)
            }
            /// `:raw-value`: the title as text.
            pub fn raw_value(&self) -> String {
                self.title()
                    .map(|t| org_trim(&text(&t)).to_string())
                    .unwrap_or_default()
            }
            /// `:pre-blank`: blank lines between the headline and its contents.
            pub fn pre_blank(&self) -> usize {
                if contents_range(&self.0).is_none() {
                    return 0;
                }
                self.0
                    .children_with_tokens()
                    .take_while(|e| !e.kind().is_element())
                    .filter(|e| e.kind() == BLANK_LINE)
                    .count()
            }
            /// The planning line, if any.
            pub fn planning(&self) -> Option<Planning> {
                self.section_like()
                    .and_then(|s| s.children().next())
                    .and_then(Planning::cast)
            }
            /// The property drawer, if any.
            pub fn property_drawer(&self) -> Option<PropertyDrawer> {
                let s = self.section_like()?;
                let mut it = s.children().filter(|c| c.kind().is_element());
                let first = it.next()?;
                if first.kind() == PLANNING {
                    it.next().and_then(PropertyDrawer::cast)
                } else {
                    PropertyDrawer::cast(first)
                }
            }
            /// Properties from the property drawer, in order, with keys as
            /// written.
            pub fn properties(&self) -> Vec<(String, String)> {
                self.property_drawer()
                    .map(|d| d.properties().map(|p| (p.key(), p.value())).collect())
                    .unwrap_or_default()
            }
        }
    };
}

heading_accessors!(Headline);
heading_accessors!(Inlinetask);

impl Headline {
    fn section_like(&self) -> Option<SyntaxNode> {
        child(&self.0, SECTION)
    }
    /// The section directly under the headline.
    pub fn section(&self) -> Option<Section> {
        self.section_like().and_then(Section::cast)
    }
    /// Direct child headlines.
    pub fn children(&self) -> impl Iterator<Item = Headline> + '_ {
        self.0.children().filter_map(Headline::cast)
    }
    /// `:footnote-section-p`.
    pub fn is_footnote_section(&self, ctx: &ParseContext) -> bool {
        ctx.footnote_section
            .as_deref()
            .is_some_and(|s| s == self.raw_value())
    }
}

impl Inlinetask {
    fn section_like(&self) -> Option<SyntaxNode> {
        Some(self.0.clone())
    }
}

impl Document {
    /// Top-level headlines.
    pub fn headlines(&self) -> impl Iterator<Item = Headline> + '_ {
        self.0.children().filter_map(Headline::cast)
    }
    /// The section before the first headline.
    pub fn first_section(&self) -> Option<Section> {
        self.0.children().find_map(Section::cast)
    }
    /// All keywords in the document, in order.
    pub fn keywords(&self) -> impl Iterator<Item = Keyword> + '_ {
        self.0.descendants().filter_map(Keyword::cast)
    }
}

impl Planning {
    fn stamp(&self, name: &str) -> Option<Timestamp> {
        let mut key: Option<String> = None;
        let mut found = None;
        for e in self.0.children_with_tokens() {
            match e {
                NodeOrToken::Token(t) if t.kind() == KEY => key = Some(t.text().to_string()),
                NodeOrToken::Node(n) if n.kind() == TIMESTAMP && key.as_deref() == Some(name) => {
                    found = Timestamp::cast(n);
                }
                _ => {}
            }
        }
        found
    }
    /// `:scheduled`.
    pub fn scheduled(&self) -> Option<Timestamp> {
        self.stamp("SCHEDULED:")
    }
    /// `:deadline`.
    pub fn deadline(&self) -> Option<Timestamp> {
        self.stamp("DEADLINE:")
    }
    /// `:closed`.
    pub fn closed(&self) -> Option<Timestamp> {
        self.stamp("CLOSED:")
    }
}

impl PropertyDrawer {
    /// The node properties.
    pub fn properties(&self) -> impl Iterator<Item = NodeProperty> + '_ {
        self.0.children().filter_map(NodeProperty::cast)
    }
}

static PROPERTY_LINE: re::Lazy =
    re::Lazy::new(r"(?m)^([ \t]*):([^{S}]+):(?:$|[ \t]+(.*?))([ \t]*)$");

impl NodeProperty {
    /// `:key`.
    pub fn key(&self) -> String {
        token(&self.0, KEY)
            .map(|t| t.text().to_string())
            .unwrap_or_default()
    }
    /// `:value`.
    pub fn value(&self) -> String {
        let t = lf(text(&self.0));
        match re::looking_at(PROPERTY_LINE.get(), &t, 0, t.len(), 0) {
            Some(m) => m
                .get(3)
                .map(|(s, e)| t[s..e].to_string())
                .unwrap_or_default(),
            None => String::new(),
        }
    }
}

impl Drawer {
    /// `:drawer-name`.
    pub fn name(&self) -> String {
        child(&self.0, BLOCK_BEGIN)
            .and_then(|b| token(&b, KEY))
            .map(|t| t.text().to_string())
            .unwrap_or_default()
    }
}

// ------------------------------------------------------------------------
// Lists.

/// `:type` of a plain list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ListType {
    /// `1.` or `a)` bullets.
    Ordered,
    /// `-`, `+` or `*` bullets.
    Unordered,
    /// Unordered items with `tag ::`.
    Descriptive,
}

impl PlainList {
    /// `:type`.
    pub fn list_type(&self) -> ListType {
        let Some(first) = self.items().next() else {
            return ListType::Unordered;
        };
        let bullet = first.bullet();
        if bullet
            .trim_start()
            .starts_with(|c: char| c.is_ascii_alphanumeric())
        {
            ListType::Ordered
        } else if child(&first.0, ITEM_TAG).is_some() {
            ListType::Descriptive
        } else {
            ListType::Unordered
        }
    }
    /// The items.
    pub fn items(&self) -> impl Iterator<Item = Item> + '_ {
        self.0.children().filter_map(Item::cast)
    }
}

/// `:checkbox` of an item.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Checkbox {
    /// `[X]`.
    On,
    /// `[ ]`.
    Off,
    /// `[-]`.
    Partial,
}

impl Item {
    /// `:bullet`, including the whitespace after it.
    pub fn bullet(&self) -> String {
        let mut out = String::new();
        let mut seen = false;
        for e in self.0.children_with_tokens() {
            match e {
                NodeOrToken::Token(t) if t.kind() == BULLET => {
                    out.push_str(t.text());
                    seen = true;
                }
                NodeOrToken::Token(t) if seen && t.kind() == WHITESPACE => {
                    out.push_str(t.text());
                    break;
                }
                _ if seen => break,
                _ => {}
            }
        }
        out
    }
    /// `:checkbox`.
    pub fn checkbox(&self) -> Option<Checkbox> {
        match token(&self.0, CHECKBOX)?.text() {
            "[X]" => Some(Checkbox::On),
            "[ ]" => Some(Checkbox::Off),
            "[-]" => Some(Checkbox::Partial),
            _ => None,
        }
    }
    /// `:counter`.
    pub fn counter(&self) -> Option<u32> {
        let t = token(&self.0, COUNTER)?;
        let inner = t.text().trim_start_matches("[@").trim_end_matches(']');
        let inner = inner
            .strip_prefix("start:")
            .or_else(|| inner.strip_prefix("START:"))
            .unwrap_or(inner);
        let c = inner.chars().next()?;
        if c.is_ascii_alphabetic() {
            Some(c.to_ascii_uppercase() as u32 - 64)
        } else {
            inner.parse().ok()
        }
    }
    /// The parsed tag of a descriptive item.
    pub fn tag(&self) -> Option<SyntaxNode> {
        child(&self.0, ITEM_TAG)
    }
    /// `:pre-blank`.
    pub fn pre_blank(&self) -> usize {
        let Some(c) = contents_range(&self.0) else {
            return 0;
        };
        let t = text(&self.0);
        let off = usize::from(c.start() - self.0.text_range().start());
        let first_line_end = t.find('\n').map_or(t.len(), |i| i + 1);
        if off < first_line_end {
            0
        } else {
            count_lines(&t[..off])
        }
    }
}

// ------------------------------------------------------------------------
// Tables.

/// `:type` of a table.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TableType {
    /// An Org table.
    Org,
    /// A table.el table.
    TableEl,
}

impl Table {
    /// `:type`.
    pub fn table_type(&self) -> TableType {
        if token(&self.0, CODE_TEXT).is_some() {
            TableType::TableEl
        } else {
            TableType::Org
        }
    }
    /// The rows.
    pub fn rows(&self) -> impl Iterator<Item = TableRow> + '_ {
        self.0.children().filter_map(TableRow::cast)
    }
    /// `:tblfm`, in the order Emacs stores it (last line first).
    pub fn tblfm(&self) -> Vec<String> {
        let mut out = Vec::new();
        for t in tokens(&self.0).filter(|t| t.kind() == KEY) {
            let line = rest_of_line(&self.0, t.text_range().end());
            let v = line
                .strip_prefix(':')
                .unwrap_or(&line)
                .trim_start_matches(' ');
            out.push(v.to_string());
        }
        out.reverse();
        out
    }
    /// `:value` of a table.el table.
    pub fn table_el_value(&self) -> Option<String> {
        token(&self.0, CODE_TEXT).map(|t| t.text().to_string())
    }
}

impl TableRow {
    /// `true` for `|---+---|` rows.
    pub fn is_rule(&self) -> bool {
        token(&self.0, MARKER).is_none()
    }
    /// The cells.
    pub fn cells(&self) -> impl Iterator<Item = TableCell> + '_ {
        self.0.children().filter_map(TableCell::cast)
    }
}

// ------------------------------------------------------------------------
// Blocks and lesser elements.

fn begin_line(node: &SyntaxNode) -> Option<(SyntaxNode, String)> {
    let b = child(node, BLOCK_BEGIN)?;
    let t = text(&b);
    Some((b, t))
}

fn header_match(node: &SyntaxNode, re: &re::Lazy) -> Option<(String, re::Captures)> {
    let (_, line) = begin_line(node)?;
    let line = line.trim_end_matches(['\n', '\r']).to_string();
    let m = re::looking_at(re.get(), &line, 0, line.len(), 0)?;
    Some((line, m))
}

fn group(line: &str, m: &re::Captures, i: usize) -> Option<String> {
    m.get(i).map(|(s, e)| line[s..e].to_string())
}

/// `org-unescape-code-in-string`.
pub fn unescape_code(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for line in s.split_inclusive('\n') {
        let indent = line.len() - line.trim_start_matches([' ', '\t']).len();
        let rest = &line[indent..];
        let commas = rest.len() - rest.trim_start_matches(',').len();
        let after = &rest[commas..];
        if commas > 0 && (after.starts_with('*') || after.starts_with("#+")) {
            out.push_str(&line[..indent + commas - 1]);
            out.push_str(after);
        } else {
            out.push_str(line);
        }
    }
    out
}

fn block_body(node: &SyntaxNode) -> String {
    lf(token(node, CODE_TEXT)
        .map(|t| t.text().to_string())
        .unwrap_or_default())
}

static SRC_BEGIN: re::Lazy = re::Lazy::new(
    r#"(?i)^[ \t]*#\+BEGIN_SRC(?: +([^{S}]+))?((?: +(?:-(?:l ".+"|[ikr])|[-+]n(?: *[0-9]+)?))+)?(.*)[ \t]*$"#,
);
static EXAMPLE_BEGIN: re::Lazy = re::Lazy::new(r"(?i)^[ \t]*#\+BEGIN_EXAMPLE(?: +(.*))?");
static EXPORT_BEGIN: re::Lazy =
    re::Lazy::new(r"(?i)[ \t]*#\+BEGIN_EXPORT(?:[ \t]+([^{S}]+))?[ \t]*$");
static SPECIAL_BEGIN: re::Lazy = re::Lazy::new(r"(?i)[ \t]*#\+BEGIN_([^{S}]+)[ \t]*(.*)[ \t]*$");
static DYNAMIC_OPEN: re::Lazy = re::Lazy::new(r"(?i)^[ \t]*#\+BEGIN:[ \t]*([{W}]+)(?:[ \t]+(.+))?");

impl SrcBlock {
    /// `:language`.
    pub fn language(&self) -> Option<String> {
        let (l, m) = header_match(&self.0, &SRC_BEGIN)?;
        group(&l, &m, 1)
    }
    /// `:switches`.
    pub fn switches(&self) -> Option<String> {
        let (l, m) = header_match(&self.0, &SRC_BEGIN)?;
        group(&l, &m, 2)
            .and_then(|s| nw(&s))
            .map(|s| org_trim(&s).to_string())
    }
    /// `:parameters`.
    pub fn parameters(&self) -> Option<String> {
        let (l, m) = header_match(&self.0, &SRC_BEGIN)?;
        group(&l, &m, 3)
            .and_then(|s| nw(&s))
            .map(|s| org_trim(&s).to_string())
    }
    /// `:value`: the code, with Org escapes removed.
    pub fn value(&self) -> String {
        unescape_code(&block_body(&self.0))
    }
}

impl ExampleBlock {
    /// `:switches`.
    pub fn switches(&self) -> Option<String> {
        let (l, m) = header_match(&self.0, &EXAMPLE_BEGIN)?;
        group(&l, &m, 1)
    }
    /// `:value`.
    pub fn value(&self) -> String {
        unescape_code(&block_body(&self.0))
    }
}

impl ExportBlock {
    /// `:type`: the back-end, upper-cased.
    pub fn backend(&self) -> Option<String> {
        let (l, m) = header_match(&self.0, &EXPORT_BEGIN)?;
        group(&l, &m, 1).map(|s| crate::tables::upcase(&s))
    }
    /// `:value`.
    pub fn value(&self) -> String {
        unescape_code(&block_body(&self.0))
    }
}

impl CommentBlock {
    /// `:value`.
    pub fn value(&self) -> String {
        block_body(&self.0)
    }
}

impl SpecialBlock {
    /// `:type`.
    pub fn block_type(&self) -> String {
        header_match(&self.0, &SPECIAL_BEGIN)
            .and_then(|(l, m)| group(&l, &m, 1))
            .unwrap_or_default()
    }
    /// `:parameters`.
    pub fn parameters(&self) -> Option<String> {
        let (l, m) = header_match(&self.0, &SPECIAL_BEGIN)?;
        group(&l, &m, 2)
            .and_then(|s| nw(&s))
            .map(|s| org_trim(&s).to_string())
    }
}

impl DynamicBlock {
    /// `:block-name`.
    pub fn block_name(&self) -> String {
        header_match(&self.0, &DYNAMIC_OPEN)
            .and_then(|(l, m)| group(&l, &m, 1))
            .unwrap_or_default()
    }
    /// `:arguments`.
    pub fn arguments(&self) -> Option<String> {
        let (l, m) = header_match(&self.0, &DYNAMIC_OPEN)?;
        group(&l, &m, 2)
    }
}

impl Keyword {
    /// `:key`, upper-cased.
    pub fn key(&self) -> String {
        token(&self.0, KEY)
            .map(|t| crate::tables::upcase(t.text()))
            .unwrap_or_default()
    }
    /// `:value`.
    pub fn value(&self) -> String {
        let Some(k) = token(&self.0, KEY) else {
            return String::new();
        };
        let line = rest_of_line(&self.0, k.text_range().end());
        org_trim(line.strip_prefix(':').unwrap_or(&line)).to_string()
    }
}

impl AffiliatedKeyword {
    /// The keyword as written, upper-cased (before translation of old
    /// names such as `SRCNAME`).
    pub fn raw_key(&self) -> String {
        token(&self.0, KEY)
            .map(|t| crate::tables::upcase(t.text()))
            .unwrap_or_default()
    }
    /// The keyword after `org-element-keyword-translation-alist`.
    pub fn key(&self) -> String {
        let k = self.raw_key();
        match k.as_str() {
            "DATA" | "LABEL" | "RESNAME" | "SOURCE" | "SRCNAME" | "TBLNAME" => "NAME".into(),
            "RESULT" => "RESULTS".into(),
            "HEADERS" => "HEADER".into(),
            _ => k,
        }
    }
    /// The main value, trimmed.
    pub fn value(&self) -> String {
        let markers = markers(&self.0);
        let Some(colon) = markers.last() else {
            return String::new();
        };
        org_trim(&rest_of_line(&self.0, colon.text_range().end())).to_string()
    }
    /// The secondary value of a dual keyword, as in `#+RESULTS[hash]:`.
    pub fn dual_value(&self) -> Option<String> {
        let m = markers(&self.0);
        if m.len() < 4 {
            return None;
        }
        let full = text(&self.0);
        let base = self.0.text_range().start();
        let s = usize::from(m[1].text_range().end() - base);
        let e = usize::from(m[2].text_range().start() - base);
        Some(full[s..e].to_string())
    }
}

/// Convenience: the `#+NAME` of an element.
pub fn element_name(node: &SyntaxNode) -> Option<String> {
    affiliated_keywords(node)
        .filter(|k| k.key() == "NAME")
        .last()
        .map(|k| k.value())
}

impl BabelCall {
    fn parts(
        &self,
    ) -> (
        Option<String>,
        Option<String>,
        Option<String>,
        Option<String>,
        String,
    ) {
        let pa = post_affiliated(&self.0);
        let t = lf(text(&self.0));
        let off = usize::from(pa - self.0.text_range().start());
        let s = &t[off..];
        let line_end = s.find('\n').unwrap_or(s.len());
        // `before-blank` is the start of the next line.
        let before_blank = if line_end < s.len() {
            line_end + 1
        } else {
            line_end
        };
        let colon = s[..line_end].find(':').map_or(before_blank, |i| i + 1);
        let mut p = colon
            + (s[colon..line_end.max(colon)].len()
                - s[colon..line_end.max(colon)]
                    .trim_start_matches([' ', '\t'])
                    .len());
        let value = org_trim(&s[p.min(line_end)..line_end]).to_string();
        let call_start = p;
        while p < before_blank && !matches!(s.as_bytes()[p], b'[' | b']' | b'(' | b')') {
            p += 1;
        }
        let call = nw(&s[call_start..p]);
        let inside = paired_str(s, &mut p, b'[', b']');
        let args = paired_str(s, &mut p, b'(', b')').and_then(|a| nw(&a));
        let rest_end = s[p..].find('\n').map_or(s.len(), |i| p + i);
        let end_header = nw(org_trim(&s[p..rest_end])).map(|x| org_trim(&x).to_string());
        (call, inside, args, end_header, value)
    }
    /// `:call`.
    pub fn call(&self) -> Option<String> {
        self.parts().0
    }
    /// `:inside-header`.
    pub fn inside_header(&self) -> Option<String> {
        self.parts().1
    }
    /// `:arguments`.
    pub fn arguments(&self) -> Option<String> {
        self.parts().2
    }
    /// `:end-header`.
    pub fn end_header(&self) -> Option<String> {
        self.parts().3
    }
    /// `:value`.
    pub fn value(&self) -> String {
        self.parts().4
    }
}

/// `org-element--parse-paired-brackets` on a string.
fn paired_str(s: &str, p: &mut usize, open: u8, close: u8) -> Option<String> {
    let b = s.as_bytes();
    if b.get(*p) != Some(&open) {
        return None;
    }
    let mut depth = 0;
    let mut i = *p;
    while i < b.len() {
        if b[i] == open {
            depth += 1;
        } else if b[i] == close {
            depth -= 1;
            if depth == 0 {
                let inner = s[*p + 1..i].to_string();
                *p = i + 1;
                return Some(inner);
            }
        }
        i += 1;
    }
    None
}

/// `:status` of a clock.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClockStatus {
    /// A clock that is still running.
    Running,
    /// A closed clock with a duration.
    Closed,
}

impl Clock {
    /// `:duration`, such as `1:30`.
    pub fn duration(&self) -> Option<String> {
        let t = text(&self.0);
        let line = t.lines().next().unwrap_or("");
        let key = line.to_ascii_uppercase().find("CLOCK:")?;
        let after = &line[key + 6..];
        let i = after.find("=> ")?;
        let rest = after[i + 3..].trim_start_matches([' ', '\t']);
        let word: String = rest
            .chars()
            .take_while(|c| !crate::tables::is_space(*c))
            .collect();
        let tail = &rest[word.len()..];
        (!word.is_empty() && tail.chars().all(|c| c == ' ' || c == '\t')).then_some(word)
    }
    /// `:status`.
    pub fn status(&self) -> ClockStatus {
        if self.duration().is_some() {
            ClockStatus::Closed
        } else {
            ClockStatus::Running
        }
    }
    /// `:value`: the clock's timestamp.
    pub fn timestamp(&self) -> Option<Timestamp> {
        self.0.children().find_map(Timestamp::cast)
    }
}

impl Comment {
    /// `:value`: the comment lines without `# `.
    pub fn value(&self) -> String {
        let pb = post_blank_text_len(&self.0);
        let t = text(&self.0);
        let body = &t[..t.len() - pb];
        body.lines()
            .map(|l| {
                let s = l.trim_start_matches([' ', '\t']);
                let s = s.strip_prefix('#').unwrap_or(s);
                s.strip_prefix(' ').unwrap_or(s).to_string()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }
}

fn post_blank_text_len(node: &SyntaxNode) -> usize {
    let all: Vec<SyntaxElement> = node.children_with_tokens().collect();
    all.iter()
        .rev()
        .take_while(|e| is_blank_token(e))
        .map(|e| usize::from(e.text_range().len()))
        .sum()
}

impl FixedWidth {
    /// `:value`.
    pub fn value(&self) -> String {
        let pa = usize::from(post_affiliated(&self.0) - self.0.text_range().start());
        let t = text(&self.0);
        let body = lf(t[pa..t.len() - post_blank_text_len(&self.0)].to_string());
        let body = body.strip_suffix('\n').unwrap_or(&body);
        body.split('\n')
            .map(|l| {
                let s = l.trim_start_matches([' ', '\t']);
                let s = s.strip_prefix(':').unwrap_or(s);
                s.strip_prefix(' ').unwrap_or(s).to_string()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }
}

impl DiarySexp {
    /// `:value`.
    pub fn value(&self) -> String {
        let pa = post_affiliated(&self.0);
        rest_of_line(&self.0, pa)
    }
}

impl LatexEnvironment {
    /// `:value`: the environment, including its final newline.
    pub fn value(&self) -> String {
        let pa = usize::from(post_affiliated(&self.0) - self.0.text_range().start());
        let t = text(&self.0);
        lf(t[pa..t.len() - post_blank_text_len(&self.0)].to_string())
    }
}

impl FootnoteDefinition {
    /// `:label`.
    pub fn label(&self) -> String {
        token(&self.0, KEY)
            .map(|t| t.text().to_string())
            .unwrap_or_default()
    }
    /// `:pre-blank`.
    pub fn pre_blank(&self) -> usize {
        let Some(c) = contents_range(&self.0) else {
            return 0;
        };
        let t = text(&self.0);
        let pa = usize::from(post_affiliated(&self.0) - self.0.text_range().start());
        let off = usize::from(c.start() - self.0.text_range().start());
        let pa_line_end = t[pa..].find('\n').map_or(t.len(), |i| pa + i + 1);
        if off < pa_line_end {
            0
        } else {
            count_lines(&t[..off])
        }
    }
}

// ------------------------------------------------------------------------
// Objects.

impl Entity {
    /// `:name`.
    pub fn name(&self) -> String {
        token(&self.0, KEY)
            .map(|t| t.text().to_string())
            .unwrap_or_default()
    }
    /// `:use-brackets-p`.
    pub fn uses_brackets(&self) -> bool {
        markers(&self.0).len() > 1
    }
    /// The entity's UTF-8 rendering.
    pub fn utf8(&self) -> Option<&'static str> {
        crate::tables::entity(&self.name()).map(|e| e.6)
    }
    /// The entity's LaTeX rendering and whether it needs math mode.
    pub fn latex(&self) -> Option<(&'static str, bool)> {
        crate::tables::entity(&self.name()).map(|e| (e.1, e.2))
    }
    /// The entity's HTML rendering.
    pub fn html(&self) -> Option<&'static str> {
        crate::tables::entity(&self.name()).map(|e| e.3)
    }
}

fn code_text(node: &SyntaxNode) -> String {
    lf(token(node, CODE_TEXT)
        .map(|t| t.text().to_string())
        .unwrap_or_default())
}

impl Code {
    /// `:value`.
    pub fn value(&self) -> String {
        code_text(&self.0)
    }
}

impl Verbatim {
    /// `:value`.
    pub fn value(&self) -> String {
        code_text(&self.0)
    }
}

impl LatexFragment {
    /// `:value`.
    pub fn value(&self) -> String {
        code_text(&self.0)
    }
}

impl ExportSnippet {
    /// `:back-end`.
    pub fn backend(&self) -> String {
        token(&self.0, KEY)
            .map(|t| t.text().to_string())
            .unwrap_or_default()
    }
    /// `:value`.
    pub fn value(&self) -> String {
        code_text(&self.0)
    }
}

impl FootnoteReference {
    /// `:label`.
    pub fn label(&self) -> Option<String> {
        token(&self.0, KEY).map(|t| t.text().to_string())
    }
    /// `true` for inline footnotes (`[fn:label:definition]`).
    pub fn is_inline(&self) -> bool {
        markers(&self.0).len() == 3
    }
}

fn inline_headers(node: &SyntaxNode) -> (usize, String) {
    let t = lf(text(node));
    let k = token(node, KEY)
        .map(|k| usize::from(k.text_range().end() - node.text_range().start()))
        .unwrap_or(0);
    (k, t)
}

fn normalize_header(s: String) -> Option<String> {
    let s = nw(&s)?;
    let trimmed = org_trim(&s);
    let mut out = String::new();
    let mut chars = trimmed.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\n' {
            while matches!(chars.peek(), Some(' ' | '\t')) {
                chars.next();
            }
            out.push(' ');
        } else {
            out.push(c);
        }
    }
    Some(out)
}

impl InlineBabelCall {
    /// `:call`.
    pub fn call(&self) -> String {
        token(&self.0, KEY)
            .map(|t| t.text().to_string())
            .unwrap_or_default()
    }
    fn parts(&self) -> (Option<String>, Option<String>, Option<String>) {
        let (mut p, t) = inline_headers(&self.0);
        let inside = paired_str(&t, &mut p, b'[', b']').and_then(normalize_header);
        let args = paired_str(&t, &mut p, b'(', b')').and_then(|a| nw(&a));
        let end = paired_str(&t, &mut p, b'[', b']').and_then(normalize_header);
        (inside, args, end)
    }
    /// `:inside-header`.
    pub fn inside_header(&self) -> Option<String> {
        self.parts().0
    }
    /// `:arguments`.
    pub fn arguments(&self) -> Option<String> {
        self.parts().1
    }
    /// `:end-header`.
    pub fn end_header(&self) -> Option<String> {
        self.parts().2
    }
}

impl InlineSrcBlock {
    /// `:language`.
    pub fn language(&self) -> String {
        token(&self.0, KEY)
            .map(|t| t.text().to_string())
            .unwrap_or_default()
    }
    /// `:parameters`.
    pub fn parameters(&self) -> Option<String> {
        let (mut p, t) = inline_headers(&self.0);
        paired_str(&t, &mut p, b'[', b']').and_then(normalize_header)
    }
    /// `:value`.
    pub fn value(&self) -> String {
        code_text(&self.0)
    }
}

impl Macro {
    /// `:key`, lower-cased.
    pub fn key(&self) -> String {
        token(&self.0, KEY)
            .map(|t| crate::tables::downcase(t.text()))
            .unwrap_or_default()
    }
    /// `:args`, as `org-macro-extract-arguments` returns them.
    pub fn args(&self) -> Vec<String> {
        let t = lf(text(&self.0));
        let Some(k) = token(&self.0, KEY) else {
            return Vec::new();
        };
        let after = usize::from(k.text_range().end() - self.0.text_range().start());
        let rest = &t[after..];
        if !rest.starts_with('(') {
            return Vec::new();
        }
        let Some(close) = rest.rfind(")}}}") else {
            return Vec::new();
        };
        let inner = &rest[1..close];
        // `(replace-regexp-in-string "[ \t\r\n]+" " " (org-trim a))`
        let mut collapsed = String::new();
        let mut in_space = false;
        for c in org_trim(inner).chars() {
            if matches!(c, ' ' | '\t' | '\r' | '\n') {
                if !in_space {
                    collapsed.push(' ');
                }
                in_space = true;
            } else {
                collapsed.push(c);
                in_space = false;
            }
        }
        // Split on unescaped commas; `\\,` is an escaped comma.
        let mut args = vec![String::new()];
        let mut backslashes = 0usize;
        for c in collapsed.chars() {
            if c == '\\' {
                backslashes += 1;
                continue;
            }
            if c == ',' {
                let keep = backslashes / 2;
                args.last_mut()
                    .expect("non-empty")
                    .push_str(&"\\".repeat(keep));
                if backslashes.is_multiple_of(2) {
                    args.push(String::new());
                } else {
                    args.last_mut().expect("non-empty").push(',');
                }
            } else {
                args.last_mut()
                    .expect("non-empty")
                    .push_str(&"\\".repeat(backslashes));
                args.last_mut().expect("non-empty").push(c);
            }
            backslashes = 0;
        }
        args.last_mut()
            .expect("non-empty")
            .push_str(&"\\".repeat(backslashes));
        args
    }
}

fn between_markers(node: &SyntaxNode) -> String {
    let m = markers(node);
    match (m.first(), m.last()) {
        (Some(a), Some(b)) if a != b => {
            let t = text(node);
            let base = node.text_range().start();
            t[usize::from(a.text_range().end() - base)..usize::from(b.text_range().start() - base)]
                .to_string()
        }
        _ => String::new(),
    }
}

impl Target {
    /// `:value`.
    pub fn value(&self) -> String {
        between_markers(&self.0)
    }
}

impl RadioTarget {
    /// `:value`.
    pub fn value(&self) -> String {
        between_markers(&self.0)
    }
}

impl StatisticsCookie {
    /// `:value`, such as `[2/5]`.
    pub fn value(&self) -> String {
        let t = text(&self.0);
        t[..t.len() - post_blank(&self.0)].to_string()
    }
}

impl Subscript {
    /// `:use-brackets-p`.
    pub fn uses_brackets(&self) -> bool {
        markers(&self.0).len() >= 3
    }
}

impl Superscript {
    /// `:use-brackets-p`.
    pub fn uses_brackets(&self) -> bool {
        markers(&self.0).len() >= 3
    }
}

impl Citation {
    /// `:style`, such as `t` in `[cite/t:@key]`.
    pub fn style(&self) -> Option<String> {
        let first = markers(&self.0).into_iter().next()?;
        let t = first.text();
        let inner = t.get(5..)?;
        let inner = inner.strip_prefix('/')?;
        let end = inner.find(':')?;
        Some(inner[..end].to_string())
    }
    /// The references.
    pub fn references(&self) -> impl Iterator<Item = CitationReference> + '_ {
        self.0.children().filter_map(CitationReference::cast)
    }
}

impl CitationReference {
    /// `:key`.
    pub fn key(&self) -> String {
        token(&self.0, KEY)
            .map(|t| t.text().to_string())
            .unwrap_or_default()
    }
}

// ------------------------------------------------------------------------
// Links.

/// `:format` of a link.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkFormat {
    /// `[[path][description]]`.
    Bracket,
    /// `https://example.com`.
    Plain,
    /// `<https://example.com>`.
    Angle,
    /// Text matching a radio target. Emacs reports these as `plain` with
    /// type `radio`.
    Radio,
}

/// The resolved properties of a link.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinkInfo {
    /// `:type`, such as `https`, `file`, `id`, `fuzzy`.
    pub link_type: String,
    /// `:path`.
    pub path: String,
    /// `:raw-link`.
    pub raw_link: String,
    /// `:format`.
    pub format: LinkFormat,
    /// `:search-option` of file links.
    pub search_option: Option<String>,
    /// `:application` of `file+app:` links.
    pub application: Option<String>,
}

impl Link {
    /// `:format`.
    pub fn format(&self) -> LinkFormat {
        let m = markers(&self.0);
        if token(&self.0, CODE_TEXT).is_none() {
            LinkFormat::Radio
        } else if m.first().is_some_and(|t| t.text() == "[[") {
            LinkFormat::Bracket
        } else if m.first().is_some_and(|t| t.text() == "<") {
            LinkFormat::Angle
        } else {
            LinkFormat::Plain
        }
    }

    /// The description, for bracket links with one.
    pub fn description(&self) -> Option<TextRange> {
        (self.format() == LinkFormat::Bracket)
            .then(|| contents_range(&self.0))
            .flatten()
    }

    /// Resolves the link's properties as `org-element-link-parser` does.
    pub fn info(&self, ctx: &ParseContext) -> LinkInfo {
        let format = self.format();
        let rx = ctx.regexes();
        let code = code_text(&self.0);
        let (mut link_type, mut path, raw_link) = match format {
            LinkFormat::Radio => {
                let t = text(&self.0);
                let v = t[..t.len() - post_blank(&self.0)].to_string();
                ("radio".to_string(), v.clone(), v)
            }
            LinkFormat::Bracket => {
                let raw = expand_abbrev(&unescape_link(&collapse_newlines(&code, " ")), ctx);
                let (ty, path) = if raw.starts_with('/')
                    || raw == "~"
                    || raw.starts_with("~/")
                    || raw.starts_with("./")
                    || raw.starts_with("../")
                {
                    ("file".to_string(), raw.clone())
                } else if let Some(m) = re::looking_at(rx.types_prefix(), &raw, 0, raw.len(), 0) {
                    let (s, e) = m.get(1).expect("type");
                    (raw[s..e].to_string(), raw[m.whole().1..].to_string())
                } else if raw.starts_with('(') && raw.ends_with(')') && raw.len() >= 2 {
                    ("coderef".to_string(), raw[1..raw.len() - 1].to_string())
                } else if let Some(rest) = raw.strip_prefix('#') {
                    ("custom-id".to_string(), rest.to_string())
                } else {
                    ("fuzzy".to_string(), raw.clone())
                };
                (ty, path, raw)
            }
            LinkFormat::Plain => {
                let colon = code.find(':').unwrap_or(0);
                (
                    code[..colon].to_string(),
                    code[colon + 1..].to_string(),
                    code.clone(),
                )
            }
            LinkFormat::Angle => {
                let colon = code.find(':').unwrap_or(0);
                (
                    code[..colon].to_string(),
                    collapse_newlines(&code[colon + 1..], ""),
                    code.clone(),
                )
            }
        };
        let mut application = None;
        let mut search_option = None;
        // `(string-match "\\`file\\(?:\\+\\(.+\\)\\)?\\'" type)`, case-insensitive.
        let is_file =
            link_type.len() >= 4 && link_type.as_bytes()[..4].eq_ignore_ascii_case(b"file");
        let rest = if is_file { &link_type[4..] } else { "" };
        if is_file
            && (rest.is_empty()
                || (rest.len() > 1 && rest.starts_with('+') && !rest.contains('\n')))
        {
            if let Some(app) = rest.strip_prefix('+') {
                application = Some(app.to_string());
            }
            {
                link_type = "file".to_string();
                if let Some(i) = path.find("::")
                    && !path[i + 2..].contains('\n')
                {
                    search_option = Some(path[i + 2..].to_string());
                    path.truncate(i);
                }
                // `\`///*\(.:\)?/` -> `\1/`
                let slashes = path.len() - path.trim_start_matches('/').len();
                if slashes >= 2 {
                    let rest = &path[slashes..];
                    let mut it = rest.chars();
                    let drive = match (it.next(), it.next(), it.next()) {
                        (Some(c), Some(':'), Some('/')) => c != '\n',
                        _ => false,
                    };
                    if drive {
                        path = rest.to_string();
                    } else if slashes >= 3 {
                        path = format!("/{rest}");
                    }
                }
            }
        }
        LinkInfo {
            link_type,
            path,
            raw_link,
            format,
            search_option,
            application,
        }
    }
}

fn collapse_newlines(s: &str, with: &str) -> String {
    // `(replace-regexp-in-string "[ \t]*\n[ \t]*" WITH s)`: only the blanks
    // around each newline are replaced.
    if !s.contains('\n') {
        return s.to_string();
    }
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find('\n') {
        out.push_str(rest[..i].trim_end_matches([' ', '\t']));
        out.push_str(with);
        rest = rest[i + 1..].trim_start_matches([' ', '\t']);
    }
    out.push_str(rest);
    out
}

/// `org-link-unescape`.
fn unescape_link(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = String::with_capacity(s.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'\\' {
            let mut j = i;
            while j < b.len() && b[j] == b'\\' {
                j += 1;
            }
            let n = j - i;
            if j == b.len() || b[j] == b'[' || b[j] == b']' {
                out.push_str(&"\\".repeat(n / 2));
            } else {
                out.push_str(&s[i..j]);
            }
            i = j;
        } else {
            let c = s[i..].chars().next().expect("char");
            out.push(c);
            i += c.len_utf8();
        }
    }
    out
}

/// `url-hexify-string`.
fn url_hexify(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// `org-link-expand-abbrev`.
fn expand_abbrev(link: &str, ctx: &ParseContext) -> String {
    if link.contains('\n') {
        return link.to_string();
    }
    let (key, tag) = match link.find(':') {
        Some(i) => {
            let rest = &link[i..];
            let tag = rest
                .strip_prefix("::")
                .or_else(|| rest.strip_prefix(':'))
                .unwrap_or("");
            (&link[..i], Some(tag))
        }
        None => (link, None),
    };
    let Some((_, rpl)) = ctx.link_abbrevs.iter().find(|(k, _)| k == key) else {
        return link.to_string();
    };
    if rpl.contains("%(") {
        return link.to_string();
    }
    let tag = tag.unwrap_or("");
    if let Some(i) = rpl.find("%s") {
        format!("{}{}{}", &rpl[..i], tag, &rpl[i + 2..])
    } else if let Some(i) = rpl.find("%h") {
        format!("{}{}{}", &rpl[..i], url_hexify(tag), &rpl[i + 2..])
    } else {
        format!("{rpl}{tag}")
    }
}

// ------------------------------------------------------------------------
// Timestamps.

/// `:type` of a timestamp.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TimestampType {
    /// `<...>`.
    Active,
    /// `<...>--<...>` or a time range in an active timestamp.
    ActiveRange,
    /// `[...]`.
    Inactive,
    /// `[...]--[...]` or a time range in an inactive timestamp.
    InactiveRange,
    /// `<%%(sexp)>`.
    Diary,
}

/// `:range-type` of a timestamp.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RangeType {
    /// Two dates joined by `--`.
    DateRange,
    /// A time range within one timestamp, such as `10:00-11:00`.
    TimeRange,
}

/// A time unit of a repeater or warning.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TimeUnit {
    /// `h`.
    Hour,
    /// `d`.
    Day,
    /// `w`.
    Week,
    /// `m`.
    Month,
    /// `y`.
    Year,
}

impl TimeUnit {
    /// Emacs compares the unit case-sensitively: `W` is not a week.
    fn from_char(c: char) -> Self {
        match c {
            'h' => TimeUnit::Hour,
            'd' => TimeUnit::Day,
            'w' => TimeUnit::Week,
            'm' => TimeUnit::Month,
            _ => TimeUnit::Year,
        }
    }
    /// The Emacs symbol name.
    pub fn name(self) -> &'static str {
        match self {
            TimeUnit::Hour => "hour",
            TimeUnit::Day => "day",
            TimeUnit::Week => "week",
            TimeUnit::Month => "month",
            TimeUnit::Year => "year",
        }
    }
}

/// `:repeater-type`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RepeaterType {
    /// `+`.
    Cumulate,
    /// `++`.
    CatchUp,
    /// `.+`.
    Restart,
}

/// A repeater such as `+1w`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Repeater {
    /// The kind of repeater.
    pub kind: RepeaterType,
    /// The interval.
    pub value: u32,
    /// The interval's unit.
    pub unit: TimeUnit,
    /// `:repeater-deadline-value` and `-unit`: the `/2d` of `+1w/2d`.
    pub deadline: Option<(u32, TimeUnit)>,
}

/// A warning delay such as `-2d`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Warning {
    /// `true` for `--2d` (only the first occurrence).
    pub first_only: bool,
    /// The delay.
    pub value: u32,
    /// The delay's unit.
    pub unit: TimeUnit,
}

/// A date and optional time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DateTime {
    /// Year.
    pub year: i32,
    /// Month, 1 to 12.
    pub month: u32,
    /// Day, 1 to 31.
    pub day: u32,
    /// Hour and minute, if the timestamp has a time.
    pub time: Option<(u32, u32)>,
}

static TS_RAW: re::Lazy = re::Lazy::new(
    r"(?i)([<\[](%%)?.*?)[\]>](?:--([\[<]([0-9]{4}-[0-9]{2}-[0-9]{2}(?: .*?)?)[\]>]))?",
);
static TS_TIME_RANGE: re::Lazy =
    re::Lazy::new(r"[012]?[0-9]:[0-5][0-9](-([012]?[0-9]):([0-5][0-9]))");
static TS_REPEATER: re::Lazy =
    re::Lazy::new(r"(?i)(\+\+|\.\+|\+)([0-9]+)([hdwmy])(?:/([0-9]+)([hdwmy]))?");
static TS_WARNING: re::Lazy = re::Lazy::new(r"(?i)(-)?-([0-9]+)([hdwmy])");
static TS_DATE: re::Lazy = re::Lazy::new(
    r"(([0-9]{4})-([0-9]{2})-([0-9]{2})( +[^\]+0-9>\r\n -]+)?( +([0-9]{1,2}):([0-9]{2}))?)",
);

fn search(re: &re::Lazy, s: &str) -> Option<re::Captures> {
    re::search_forward(re.get(), s, 0, s.len(), 0, s.len())
}

fn parse_date(s: &str) -> Option<DateTime> {
    let m = search(&TS_DATE, s)?;
    let g = |i| m.get(i).map(|(a, b)| &s[a..b]);
    Some(DateTime {
        year: g(2)?.parse().ok()?,
        month: g(3)?.parse().ok()?,
        day: g(4)?.parse().ok()?,
        time: match (g(7), g(8)) {
            (Some(h), Some(mi)) => Some((h.parse().ok()?, mi.parse().ok()?)),
            _ => None,
        },
    })
}

impl Timestamp {
    fn raw_parts(&self) -> (String, Option<re::Captures>) {
        let t = text(&self.0);
        let raw = t[..t.len() - post_blank(&self.0)].to_string();
        let m = re::looking_at(TS_RAW.get(), &raw, 0, raw.len(), 0);
        (raw, m)
    }
    /// `:raw-value`.
    pub fn raw_value(&self) -> String {
        self.raw_parts().0
    }
    fn is_diary(&self) -> bool {
        self.raw_value().get(1..3) == Some("%%")
    }
    fn date_start_str(&self) -> String {
        let (raw, m) = self.raw_parts();
        match m.and_then(|m| m.get(1)) {
            Some((s, e)) => raw[s..e].to_string(),
            None => String::new(),
        }
    }
    fn date_end_str(&self) -> Option<String> {
        let (raw, m) = self.raw_parts();
        m.and_then(|m| m.get(3)).map(|(s, e)| raw[s..e].to_string())
    }
    fn time_range(&self) -> Option<(u32, u32)> {
        let ds = self.date_start_str();
        let m = search(&TS_TIME_RANGE, &ds)?;
        let g = |i| m.get(i).map(|(a, b)| &ds[a..b]);
        Some((g(2)?.parse().ok()?, g(3)?.parse().ok()?))
    }
    /// `:type`.
    pub fn timestamp_type(&self) -> TimestampType {
        if self.is_diary() {
            return TimestampType::Diary;
        }
        let active = self.raw_value().starts_with('<');
        let range = self.date_end_str().is_some() || self.time_range().is_some();
        match (active, range) {
            (true, true) => TimestampType::ActiveRange,
            (true, false) => TimestampType::Active,
            (false, true) => TimestampType::InactiveRange,
            (false, false) => TimestampType::Inactive,
        }
    }
    /// `:range-type`.
    pub fn range_type(&self) -> Option<RangeType> {
        if self.date_end_str().is_some() {
            Some(RangeType::DateRange)
        } else if self.time_range().is_some() {
            Some(RangeType::TimeRange)
        } else {
            None
        }
    }
    /// `:repeater-type`, `:repeater-value` and `:repeater-unit`.
    pub fn repeater(&self) -> Option<Repeater> {
        if self.is_diary() {
            return None;
        }
        let raw = self.raw_value();
        let m = search(&TS_REPEATER, &raw)?;
        let g = |i| m.get(i).map(|(a, b)| &raw[a..b]);
        Some(Repeater {
            kind: match g(1)? {
                "++" => RepeaterType::CatchUp,
                ".+" => RepeaterType::Restart,
                _ => RepeaterType::Cumulate,
            },
            value: g(2)?.parse().ok()?,
            unit: TimeUnit::from_char(g(3)?.chars().next()?),
            deadline: match (g(4), g(5)) {
                (Some(v), Some(u)) => {
                    Some((v.parse().ok()?, TimeUnit::from_char(u.chars().next()?)))
                }
                _ => None,
            },
        })
    }
    /// `:warning-type`, `:warning-value` and `:warning-unit`.
    pub fn warning(&self) -> Option<Warning> {
        if self.is_diary() {
            return None;
        }
        let raw = self.raw_value();
        let m = search(&TS_WARNING, &raw)?;
        let g = |i| m.get(i).map(|(a, b)| &raw[a..b]);
        Some(Warning {
            first_only: g(1).is_some(),
            value: g(2)?.parse().ok()?,
            unit: TimeUnit::from_char(g(3)?.chars().next()?),
        })
    }
    /// The start date (not for diary timestamps).
    pub fn start(&self) -> Option<DateTime> {
        if self.is_diary() {
            None
        } else {
            parse_date(&self.date_start_str())
        }
    }
    /// The end date: the second timestamp of a date range, the end of a
    /// time range, or the start.
    pub fn end(&self) -> Option<DateTime> {
        let start = self.start()?;
        if let Some(e) = self.date_end_str().and_then(|s| parse_date(&s)) {
            return Some(e);
        }
        match self.time_range() {
            Some((h, m)) => Some(DateTime {
                time: Some((h, m)),
                ..start
            }),
            None => Some(start),
        }
    }
}

impl crate::objects::ContextRegexes {
    pub(crate) fn types_prefix(&self) -> &regex_automata::meta::Regex {
        &self.types_prefix
    }
}
