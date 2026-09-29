//! Token and node kinds.

/// The kind of a token or node in the syntax tree.
#[allow(non_camel_case_types, missing_docs)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(u16)]
pub enum SyntaxKind {
    // Tokens.
    /// Characters with no special meaning.
    TEXT = 0,
    /// Spaces and tabs.
    WHITESPACE,
    /// One line ending.
    NEWLINE,
    /// A line ending followed by blank lines: the end of a paragraph.
    PAR_BREAK,
    /// `%` to the end of the line.
    COMMENT,
    /// `\name`.
    CONTROL_WORD,
    /// `\` and one character that is not a letter: `\\`, `\%`, `\(`.
    CONTROL_SYMBOL,
    L_BRACE,
    R_BRACE,
    L_BRACKET,
    R_BRACKET,
    DOLLAR,
    /// `$$`.
    DOUBLE_DOLLAR,
    AMPERSAND,
    HASH,
    CARET,
    UNDERSCORE,
    TILDE,
    /// The star of a starred command or environment (`\section*`).
    STAR,
    /// Text taken as it is: the body of `verbatim`, the argument of
    /// `\verb` and `\url`.
    VERBATIM,
    /// The name of an environment in `\begin{…}` and `\end{…}`.
    ENV_NAME,

    // Nodes.
    /// The document.
    ROOT,
    /// A run of text between paragraph breaks, in the document or in an
    /// environment.
    PARAGRAPH,
    /// A command with its arguments.
    COMMAND,
    /// An optional argument, `[…]`.
    OPT_ARG,
    /// A group or a mandatory argument, `{…}`.
    GROUP,
    /// `\begin{name}` … `\end{name}`.
    ENVIRONMENT,
    /// `\begin{name}` with the environment's arguments.
    BEGIN,
    /// `\end{name}`.
    END,
    /// What an environment contains.
    BODY,
    /// `$…$` or `\(…\)`.
    INLINE_MATH,
    /// `$$…$$` or `\[…\]`.
    DISPLAY_MATH,
    /// `\verb|…|` or `\lstinline|…|`.
    VERB,
}

impl SyntaxKind {
    /// The last kind.
    pub const LAST: SyntaxKind = SyntaxKind::VERB;

    /// Whether this is a token kind.
    pub fn is_token(self) -> bool {
        (self as u16) < (SyntaxKind::ROOT as u16)
    }
}

const KINDS: [SyntaxKind; SyntaxKind::LAST as usize + 1] = {
    use SyntaxKind::*;
    [
        TEXT,
        WHITESPACE,
        NEWLINE,
        PAR_BREAK,
        COMMENT,
        CONTROL_WORD,
        CONTROL_SYMBOL,
        L_BRACE,
        R_BRACE,
        L_BRACKET,
        R_BRACKET,
        DOLLAR,
        DOUBLE_DOLLAR,
        AMPERSAND,
        HASH,
        CARET,
        UNDERSCORE,
        TILDE,
        STAR,
        VERBATIM,
        ENV_NAME,
        ROOT,
        PARAGRAPH,
        COMMAND,
        OPT_ARG,
        GROUP,
        ENVIRONMENT,
        BEGIN,
        END,
        BODY,
        INLINE_MATH,
        DISPLAY_MATH,
        VERB,
    ]
};

/// The rowan language tag for LaTeX.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum LatexLanguage {}

impl rowan::Language for LatexLanguage {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kinds_in_order() {
        for (i, k) in KINDS.iter().enumerate() {
            assert_eq!(*k as usize, i);
        }
    }
}
