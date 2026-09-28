//! A lossless, error tolerant, incremental parser for [Org mode] files.
//!
//! `org-syntax` reads Org documents the way Emacs does. It follows
//! `org-element.el` (Org 9.7) function by function, and its output is
//! compared with Emacs on the Org manual, all of Worg and hundreds of
//! thousands of randomly mutated files: every element and object, every
//! boundary and every property agrees.
//!
//! - **Lossless.** The syntax tree contains every byte of the input:
//!   `parse(text).syntax().to_string() == text` for every input.
//! - **Error tolerant.** There is no invalid input. Malformed constructs
//!   are parsed as Emacs parses them (usually as text), and
//!   [`Parse::diagnostics`] reports likely mistakes.
//! - **Incremental.** [`Parse::reparse`] updates the tree after an edit by
//!   parsing only the affected elements; the result is identical to a full
//!   parse.
//! - **Typed.** The [`ast`] module has a wrapper for each element and object
//!   type with accessors for their properties.
//! - **Unbounded.** Nesting, file size and the number of radio targets are
//!   not limited by the implementation limits Emacs has.
//!
//! # Example
//!
//! ```
//! use org_syntax::ast::{AstNode, Headline};
//!
//! let text = "#+TODO: TODO NEXT | DONE\n* NEXT Write the parser :work:\nSome *bold* text.\n";
//! let parse = org_syntax::parse(text);
//! assert_eq!(parse.syntax().to_string(), text);
//!
//! let headline = parse.syntax().descendants().find_map(Headline::cast).unwrap();
//! assert_eq!(headline.todo_keyword().unwrap().text(), "NEXT");
//! assert_eq!(headline.tags(), vec!["work".to_string()]);
//! assert_eq!(headline.raw_value(), "Write the parser");
//! ```
//!
//! # Incremental reparsing
//!
//! ```
//! use org_syntax::{TextEdit, TextRange, TextSize};
//!
//! let old = org_syntax::parse("* A\nfirst paragraph\n\nsecond paragraph\n");
//! let edit = TextEdit { range: TextRange::empty(TextSize::from(9)), insert: "!".into() };
//! let new_text = edit.apply(&old.syntax().to_string());
//! let new = old.reparse(&new_text, &edit);
//! assert_eq!(new.green(), org_syntax::parse(&new_text).green());
//! ```
//!
//! [Org mode]: https://orgmode.org

#![warn(missing_docs)]

pub mod ast;
mod buf;
mod cache;
mod context;
mod crlf;
mod elements;
mod incremental;
mod kind;
mod lint;
mod lists;
mod objects;
mod prepass;
mod radio;
mod raw;
mod re;
mod tables;

pub mod debug;

pub use context::{
    DEFAULT_LINK_TYPES, ItemTerminator, ParseContext, TodoKeyword, TodoSequence, TodoSequenceKind,
};
pub use elements::MAX_DEPTH;
pub use incremental::{ReparseLevel, TextEdit};
pub use kind::{OrgLanguage, SyntaxKind};
pub use lint::{Diagnostic, Severity};
pub use lists::{ListItem, list_structure};
pub use prepass::{FsSetupFiles, NoSetupFiles, SetupFileLoader};
pub use rowan::{NodeOrToken, TextRange, TextSize};

/// A node in the syntax tree.
pub type SyntaxNode = rowan::SyntaxNode<OrgLanguage>;
/// A token in the syntax tree.
pub type SyntaxToken = rowan::SyntaxToken<OrgLanguage>;
/// A node or a token.
pub type SyntaxElement = rowan::SyntaxElement<OrgLanguage>;

/// The result of parsing a document.
#[derive(Debug, Clone)]
pub struct Parse {
    green: rowan::GreenNode,
    context: std::sync::Arc<ParseContext>,
    source: ContextSource,
    /// For documents with CRLF line endings or a byte order mark: the tree
    /// of the normalized text, for incremental reparsing.
    norm: Option<std::sync::Arc<crlf::Norm>>,
}

/// Where a parse's context came from, so reparsing can recompute it.
#[derive(Clone)]
pub(crate) enum ContextSource {
    /// Given by the caller ([`parse_with`]).
    Explicit,
    /// Computed from the document's own keywords on top of `base`.
    Document {
        base: std::sync::Arc<ParseContext>,
        loader: Option<std::sync::Arc<dyn SetupFileLoader + Send + Sync>>,
    },
}

impl std::fmt::Debug for ContextSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ContextSource::Explicit => f.write_str("Explicit"),
            ContextSource::Document { loader, .. } => {
                write!(
                    f,
                    "Document {{ setup files: {} }}",
                    if loader.is_some() { "yes" } else { "no" }
                )
            }
        }
    }
}

impl Parse {
    /// The root node, of kind [`SyntaxKind::DOCUMENT`].
    pub fn syntax(&self) -> SyntaxNode {
        SyntaxNode::new_root(self.green.clone())
    }

    /// The green tree, for sharing between threads.
    pub fn green(&self) -> &rowan::GreenNode {
        &self.green
    }

    /// The context the document was parsed with, including its in-buffer
    /// settings.
    pub fn context(&self) -> &ParseContext {
        &self.context
    }

    /// The document's keywords (`#+KEY: value`) in order, as
    /// `org-collect-keywords` finds them: keys upcased, values trimmed, and
    /// the keywords of each `#+SETUPFILE` inserted before that
    /// `#+SETUPFILE` entry (setup files are read through the loader the
    /// document was parsed with).
    pub fn keywords(&self) -> Vec<(String, String)> {
        // Keywords are elements: walk the element containers of the tree.
        let mut found: Vec<(String, String)> = Vec::new();
        let mut stack = vec![self.syntax()];
        while let Some(n) = stack.pop() {
            if let Some(k) = <ast::Keyword as ast::AstNode>::cast(n.clone()) {
                found.push((k.key(), k.value()));
                continue;
            }
            let children: Vec<SyntaxNode> = n
                .children()
                .filter(|c| c.kind() == SyntaxKind::KEYWORD || c.kind().is_greater_element())
                .collect();
            stack.extend(children.into_iter().rev());
        }
        self.with_setup_files(found)
    }

    /// `keywords`, a list of the document's keywords in order, with the
    /// keywords of each `#+SETUPFILE` inserted before that entry (see
    /// [`Parse::keywords`]). For callers that collect keywords themselves.
    pub fn with_setup_files(&self, keywords: Vec<(String, String)>) -> Vec<(String, String)> {
        let loader: &dyn SetupFileLoader = match &self.source {
            ContextSource::Document {
                loader: Some(l), ..
            } => l.as_ref(),
            _ => &NoSetupFiles,
        };
        prepass::expand_setupfiles(keywords, &self.context, loader)
    }
}

/// Parses `text` with the default configuration and the document's own
/// in-buffer settings.
pub fn parse(text: &str) -> Parse {
    parse_document(text, std::sync::Arc::new(ParseContext::default()), None)
}

/// Parses the file at `path` whose contents are `text`, reading its
/// `#+SETUPFILE` files from disk.
pub fn parse_file(text: &str, path: &std::path::Path) -> Parse {
    let dir = path.parent().map(|p| p.to_path_buf()).unwrap_or_default();
    let loader: std::sync::Arc<dyn SetupFileLoader + Send + Sync> =
        std::sync::Arc::new(FsSetupFiles { base: dir });
    parse_document(
        text,
        std::sync::Arc::new(ParseContext::default()),
        Some(loader),
    )
}

/// Parses `text` with `base` as the configuration, applying the document's
/// in-buffer settings on top, and reading `#+SETUPFILE` through `loader`.
pub fn parse_with_base(
    text: &str,
    base: &ParseContext,
    loader: Option<std::sync::Arc<dyn SetupFileLoader + Send + Sync>>,
) -> Parse {
    parse_document(text, std::sync::Arc::new(base.clone()), loader)
}

pub(crate) fn parse_document(
    text: &str,
    base: std::sync::Arc<ParseContext>,
    loader: Option<std::sync::Arc<dyn SetupFileLoader + Send + Sync>>,
) -> Parse {
    let (raw, ctx, norm) = match &loader {
        Some(l) => prepass::parse_document(text, &base, l.as_ref()),
        None => prepass::parse_document(text, &base, &NoSetupFiles),
    };
    Parse {
        green: raw::build(&raw, text),
        context: std::sync::Arc::new(ctx),
        source: ContextSource::Document { base, loader },
        norm: norm.map(std::sync::Arc::new),
    }
}

/// Parses `text` with exactly the given context (in-buffer settings are not
/// applied; see [`ParseContext::for_document`]).
pub fn parse_with(text: &str, ctx: &ParseContext) -> Parse {
    let (raw, norm) = parse_raw(text, ctx);
    Parse {
        green: raw::build(&raw, text),
        context: std::sync::Arc::new(ctx.clone()),
        source: ContextSource::Explicit,
        norm: norm.map(std::sync::Arc::new),
    }
}

/// Runs `f`, growing the stack first if little of it is left. Org nesting
/// has no depth limit, so every recursive walk goes through this.
#[inline]
pub(crate) fn deep<R>(f: impl FnOnce() -> R) -> R {
    stacker::maybe_grow(128 * 1024, 8 * 1024 * 1024, f)
}

/// Parses `text`, normalizing CRLF line endings and a byte order mark
/// first. For such documents it also returns the normalized tree.
pub(crate) fn parse_raw(text: &str, ctx: &ParseContext) -> (raw::Raw, Option<crlf::Norm>) {
    match crlf::normalize(text) {
        None => (elements::Parser::new(text, ctx).parse_document(), None),
        Some((normalized, map)) => {
            let mut raw = elements::Parser::new(&normalized, ctx).parse_document();
            let green = raw::build_green_mode(&raw, &normalized, false);
            map.apply(&mut raw);
            (
                raw,
                Some(crlf::Norm {
                    green,
                    map,
                    text: normalized,
                }),
            )
        }
    }
}

#[cfg(test)]
mod tests;
