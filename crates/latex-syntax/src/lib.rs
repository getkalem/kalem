//! A lossless, error tolerant, incremental parser for LaTeX documents
//! (design §9.5, T2.7h.2).
//!
//! LaTeX has no reference grammar: TeX is a macro language. This parser
//! reads the conventions documents follow, with TeX's standard category
//! codes: commands with the arguments of a signature table
//! ([`signatures`]), groups, environments nested by name, math in `$…$`,
//! `\(…\)`, `\[…\]`, `$$…$$` and the math environments, comments, and
//! verbatim text (`\verb`, `verbatim`, `lstlisting`, `minted`, `\url`)
//! taken whole. It returns ranges into the text: the tree's text is the
//! input, byte for byte, whatever the input.
//!
//! Unbalanced input is closed where a reader would close it: a group left
//! open ends at the paragraph, an environment left open at the next
//! sectioning command, math at the paragraph; each is reported in
//! [`Parse::diagnostics`]. Nothing panics (checked by property tests on
//! random input) and the stack grows for deep nesting.
//!
//! [`Parse::reparse`] parses again only the paragraph around an edit, in
//! the innermost environment containing it, when that gives the tree a
//! full parse gives; otherwise it parses everything.

mod incremental;
mod kind;
mod lexer;
mod parser;
pub mod signatures;
mod tables;

use std::ops::Range;

pub use kind::{LatexLanguage, SyntaxKind};
pub use rowan::{GreenNode, TextRange, TextSize};

/// A node of the syntax tree.
pub type SyntaxNode = rowan::SyntaxNode<LatexLanguage>;
/// A token of the syntax tree.
pub type SyntaxToken = rowan::SyntaxToken<LatexLanguage>;
/// A node or a token.
pub type SyntaxElement = rowan::SyntaxElement<LatexLanguage>;

/// Something the parser closed or skipped for the author to look at.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    /// Where, in bytes.
    pub range: Range<usize>,
    /// What.
    pub message: String,
}

/// An edit of the text: `range` of the old text replaced by `insert`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextEdit {
    /// The replaced range, in the old text.
    pub range: Range<usize>,
    /// The new text.
    pub insert: String,
}

impl TextEdit {
    /// Applies the edit to `text`.
    pub fn apply(&self, text: &str) -> String {
        let mut s = String::with_capacity(text.len() + self.insert.len());
        s.push_str(&text[..self.range.start]);
        s.push_str(&self.insert);
        s.push_str(&text[self.range.end..]);
        s
    }
}

/// A parsed document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Parse {
    green: GreenNode,
    diagnostics: Vec<Diagnostic>,
    toggles: Vec<(usize, bool)>,
    unclosed_env: bool,
}

/// Parses `text`.
pub fn parse(text: &str) -> Parse {
    let p = parser::Parser::new(text, 0, text.len(), false);
    let (green, p) = p.finish(SyntaxKind::ROOT, text.len(), parser::Mode::Text);
    Parse {
        green,
        toggles: p.toggles().to_vec(),
        diagnostics: p.diagnostics,
        unclosed_env: p.unclosed_env,
    }
}

impl Parse {
    /// The root of the tree.
    pub fn syntax(&self) -> SyntaxNode {
        SyntaxNode::new_root(self.green.clone())
    }

    /// The green tree, shared between versions.
    pub fn green(&self) -> &GreenNode {
        &self.green
    }

    /// What the parser closed or skipped, in order.
    pub fn diagnostics(&self) -> &[Diagnostic] {
        &self.diagnostics
    }

    /// The parse of `new_text`, which is the old text with `edit` applied:
    /// incremental where it can be, always the same as [`parse`].
    pub fn reparse(&self, new_text: &str, edit: &TextEdit) -> Parse {
        self.reparse_incremental(new_text, edit)
            .unwrap_or_else(|| parse(new_text))
    }

    /// The incremental part of [`Parse::reparse`]: `None` when the edit
    /// needs a full parse.
    pub fn reparse_incremental(&self, new_text: &str, edit: &TextEdit) -> Option<Parse> {
        incremental::reparse(self, new_text, edit)
    }
}

/// The name of a command (without the backslash) or an environment (for
/// `ENVIRONMENT`, `BEGIN` and `END` nodes).
pub fn name(node: &SyntaxNode) -> Option<String> {
    use SyntaxKind::*;
    match node.kind() {
        COMMAND | VERB => node
            .children_with_tokens()
            .filter_map(|e| e.into_token())
            .find(|t| matches!(t.kind(), CONTROL_WORD | CONTROL_SYMBOL))
            .map(|t| t.text()[1..].to_string()),
        ENVIRONMENT | BEGIN | END => node
            .descendants_with_tokens()
            .filter_map(|e| e.into_token())
            .find(|t| t.kind() == ENV_NAME)
            .map(|t| t.text().to_string()),
        _ => None,
    }
}
