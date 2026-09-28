//! Token and node kinds.

/// The kind of a token or node in the syntax tree.
///
/// Node kinds mirror the element and object types of Emacs's
/// `org-element.el` (for example [`SyntaxKind::HEADLINE`] is `headline`),
/// so trees can be compared with Emacs directly. A few extra node kinds
/// give structure to syntax that Emacs keeps in properties, such as
/// [`SyntaxKind::HEADLINE_TITLE`] and [`SyntaxKind::AFFILIATED_KEYWORD`].
#[allow(non_camel_case_types, missing_docs)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(u16)]
pub enum SyntaxKind {
    // Tokens.
    /// Plain text.
    TEXT = 0,
    /// Spaces and tabs.
    WHITESPACE,
    /// A line feed.
    NEWLINE,
    /// A whole blank line, including its line feed (if any).
    BLANK_LINE,
    /// Syntax characters that a WYSIWYG view hides: emphasis markers,
    /// link brackets, `#+`, drawer colons and similar.
    MARKER,
    /// The stars of a headline or inlinetask.
    STARS,
    /// A TODO keyword in a headline.
    TODO_KEYWORD,
    /// A priority cookie such as `[#A]`.
    PRIORITY,
    /// The `COMMENT` keyword of a commented headline.
    COMMENT_KEYWORD,
    /// Headline tags including the surrounding colons, such as `:a:b:`.
    TAGS,
    /// The name of a keyword, block, drawer or affiliated keyword.
    KEY,
    /// A list item bullet, such as `-` or `1.`.
    BULLET,
    /// A list item checkbox, such as `[X]`.
    CHECKBOX,
    /// A list item counter, such as `[@3]`.
    COUNTER,
    /// Raw, unparsed content: source code, verbatim text, link paths.
    CODE_TEXT,
    /// A UTF-8 byte order mark at the start of the file.
    BOM,

    // Node kinds shared with org-element.
    DOCUMENT,
    SECTION,
    HEADLINE,
    INLINETASK,
    PLANNING,
    PROPERTY_DRAWER,
    NODE_PROPERTY,
    DRAWER,
    PLAIN_LIST,
    ITEM,
    TABLE,
    TABLE_ROW,
    CENTER_BLOCK,
    QUOTE_BLOCK,
    SPECIAL_BLOCK,
    DYNAMIC_BLOCK,
    FOOTNOTE_DEFINITION,
    BABEL_CALL,
    CLOCK,
    COMMENT,
    COMMENT_BLOCK,
    DIARY_SEXP,
    EXAMPLE_BLOCK,
    EXPORT_BLOCK,
    FIXED_WIDTH,
    HORIZONTAL_RULE,
    KEYWORD,
    LATEX_ENVIRONMENT,
    PARAGRAPH,
    SRC_BLOCK,
    VERSE_BLOCK,

    BOLD,
    ITALIC,
    UNDERLINE,
    STRIKE_THROUGH,
    CODE,
    VERBATIM,
    CITATION,
    CITATION_REFERENCE,
    ENTITY,
    EXPORT_SNIPPET,
    FOOTNOTE_REFERENCE,
    INLINE_BABEL_CALL,
    INLINE_SRC_BLOCK,
    LATEX_FRAGMENT,
    LINE_BREAK,
    LINK,
    MACRO,
    RADIO_TARGET,
    STATISTICS_COOKIE,
    SUBSCRIPT,
    SUPERSCRIPT,
    TABLE_CELL,
    TARGET,
    TIMESTAMP,

    // Structural nodes without an org-element counterpart.
    /// An affiliated keyword line such as `#+NAME: x` attached to the
    /// following element.
    AFFILIATED_KEYWORD,
    /// The parsed title of a headline or inlinetask.
    HEADLINE_TITLE,
    /// The parsed tag of a descriptive list item.
    ITEM_TAG,
    /// The common prefix of a citation, or the prefix of a reference.
    CITATION_PREFIX,
    /// The common suffix of a citation, or the suffix of a reference.
    CITATION_SUFFIX,
    /// The opening line of a block, drawer or dynamic block.
    BLOCK_BEGIN,
    /// The closing line of a block, drawer or dynamic block.
    BLOCK_END,
    /// The parsed value of an affiliated keyword such as `#+CAPTION`.
    KEYWORD_VALUE,
}

impl SyntaxKind {
    /// The last variant, used for bounds checks.
    pub(crate) const LAST: SyntaxKind = SyntaxKind::KEYWORD_VALUE;

    /// Returns `true` for token kinds.
    pub fn is_token(self) -> bool {
        (self as u16) <= (SyntaxKind::BOM as u16)
    }

    /// Returns the org-element type name for kinds that have one.
    pub fn org_element_type(self) -> Option<&'static str> {
        use SyntaxKind::*;
        Some(match self {
            DOCUMENT => "org-data",
            SECTION => "section",
            HEADLINE => "headline",
            INLINETASK => "inlinetask",
            PLANNING => "planning",
            PROPERTY_DRAWER => "property-drawer",
            NODE_PROPERTY => "node-property",
            DRAWER => "drawer",
            PLAIN_LIST => "plain-list",
            ITEM => "item",
            TABLE => "table",
            TABLE_ROW => "table-row",
            CENTER_BLOCK => "center-block",
            QUOTE_BLOCK => "quote-block",
            SPECIAL_BLOCK => "special-block",
            DYNAMIC_BLOCK => "dynamic-block",
            FOOTNOTE_DEFINITION => "footnote-definition",
            BABEL_CALL => "babel-call",
            CLOCK => "clock",
            COMMENT => "comment",
            COMMENT_BLOCK => "comment-block",
            DIARY_SEXP => "diary-sexp",
            EXAMPLE_BLOCK => "example-block",
            EXPORT_BLOCK => "export-block",
            FIXED_WIDTH => "fixed-width",
            HORIZONTAL_RULE => "horizontal-rule",
            KEYWORD => "keyword",
            LATEX_ENVIRONMENT => "latex-environment",
            PARAGRAPH => "paragraph",
            SRC_BLOCK => "src-block",
            VERSE_BLOCK => "verse-block",
            BOLD => "bold",
            ITALIC => "italic",
            UNDERLINE => "underline",
            STRIKE_THROUGH => "strike-through",
            CODE => "code",
            VERBATIM => "verbatim",
            CITATION => "citation",
            CITATION_REFERENCE => "citation-reference",
            ENTITY => "entity",
            EXPORT_SNIPPET => "export-snippet",
            FOOTNOTE_REFERENCE => "footnote-reference",
            INLINE_BABEL_CALL => "inline-babel-call",
            INLINE_SRC_BLOCK => "inline-src-block",
            LATEX_FRAGMENT => "latex-fragment",
            LINE_BREAK => "line-break",
            LINK => "link",
            MACRO => "macro",
            RADIO_TARGET => "radio-target",
            STATISTICS_COOKIE => "statistics-cookie",
            SUBSCRIPT => "subscript",
            SUPERSCRIPT => "superscript",
            TABLE_CELL => "table-cell",
            TARGET => "target",
            TIMESTAMP => "timestamp",
            _ => return None,
        })
    }

    /// Returns `true` for org-element "greater elements", which contain
    /// other elements.
    pub fn is_greater_element(self) -> bool {
        use SyntaxKind::*;
        matches!(
            self,
            CENTER_BLOCK
                | DRAWER
                | DYNAMIC_BLOCK
                | FOOTNOTE_DEFINITION
                | HEADLINE
                | INLINETASK
                | ITEM
                | PLAIN_LIST
                | PROPERTY_DRAWER
                | QUOTE_BLOCK
                | SECTION
                | SPECIAL_BLOCK
                | TABLE
                | DOCUMENT
        )
    }

    /// Returns `true` for org-element element kinds.
    pub fn is_element(self) -> bool {
        (self as u16) >= (SyntaxKind::DOCUMENT as u16)
            && (self as u16) <= (SyntaxKind::VERSE_BLOCK as u16)
    }

    /// Returns `true` for org-element object kinds.
    pub fn is_object(self) -> bool {
        (self as u16) >= (SyntaxKind::BOLD as u16)
            && (self as u16) <= (SyntaxKind::TIMESTAMP as u16)
    }
}

/// The rowan language tag for Org.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum OrgLanguage {}

impl rowan::Language for OrgLanguage {
    type Kind = SyntaxKind;

    fn kind_from_raw(raw: rowan::SyntaxKind) -> SyntaxKind {
        assert!(
            raw.0 <= SyntaxKind::LAST as u16,
            "invalid syntax kind {}",
            raw.0
        );
        KINDS[raw.0 as usize]
    }

    fn kind_to_raw(kind: SyntaxKind) -> rowan::SyntaxKind {
        rowan::SyntaxKind(kind as u16)
    }
}

impl From<SyntaxKind> for rowan::SyntaxKind {
    fn from(kind: SyntaxKind) -> Self {
        rowan::SyntaxKind(kind as u16)
    }
}

/// All kinds in discriminant order, for safe conversion from raw values.
static KINDS: [SyntaxKind; SyntaxKind::LAST as usize + 1] = {
    use SyntaxKind::*;
    [
        TEXT,
        WHITESPACE,
        NEWLINE,
        BLANK_LINE,
        MARKER,
        STARS,
        TODO_KEYWORD,
        PRIORITY,
        COMMENT_KEYWORD,
        TAGS,
        KEY,
        BULLET,
        CHECKBOX,
        COUNTER,
        CODE_TEXT,
        BOM,
        DOCUMENT,
        SECTION,
        HEADLINE,
        INLINETASK,
        PLANNING,
        PROPERTY_DRAWER,
        NODE_PROPERTY,
        DRAWER,
        PLAIN_LIST,
        ITEM,
        TABLE,
        TABLE_ROW,
        CENTER_BLOCK,
        QUOTE_BLOCK,
        SPECIAL_BLOCK,
        DYNAMIC_BLOCK,
        FOOTNOTE_DEFINITION,
        BABEL_CALL,
        CLOCK,
        COMMENT,
        COMMENT_BLOCK,
        DIARY_SEXP,
        EXAMPLE_BLOCK,
        EXPORT_BLOCK,
        FIXED_WIDTH,
        HORIZONTAL_RULE,
        KEYWORD,
        LATEX_ENVIRONMENT,
        PARAGRAPH,
        SRC_BLOCK,
        VERSE_BLOCK,
        BOLD,
        ITALIC,
        UNDERLINE,
        STRIKE_THROUGH,
        CODE,
        VERBATIM,
        CITATION,
        CITATION_REFERENCE,
        ENTITY,
        EXPORT_SNIPPET,
        FOOTNOTE_REFERENCE,
        INLINE_BABEL_CALL,
        INLINE_SRC_BLOCK,
        LATEX_FRAGMENT,
        LINE_BREAK,
        LINK,
        MACRO,
        RADIO_TARGET,
        STATISTICS_COOKIE,
        SUBSCRIPT,
        SUPERSCRIPT,
        TABLE_CELL,
        TARGET,
        TIMESTAMP,
        AFFILIATED_KEYWORD,
        HEADLINE_TITLE,
        ITEM_TAG,
        CITATION_PREFIX,
        CITATION_SUFFIX,
        BLOCK_BEGIN,
        BLOCK_END,
        KEYWORD_VALUE,
    ]
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kinds_table_matches_discriminants() {
        for (i, k) in KINDS.iter().enumerate() {
            assert_eq!(*k as usize, i, "{k:?}");
        }
    }
}
