//! Footnotes: which definition a reference points to, as
//! `org-footnote-get-definition` finds it.

use org_syntax::ast::{AstNode, FootnoteDefinition, FootnoteReference};

use crate::Document;

/// A footnote reference and its definition.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FootnoteRef {
    /// The reference's start.
    pub begin: usize,
    /// Its label, if any (`None` for anonymous `[fn:: ...]`).
    pub label: Option<String>,
    /// Whether it defines the footnote inline (`[fn:x: text]`).
    pub inline: bool,
    /// The start of the definition, if found.
    pub definition: Option<usize>,
}

impl Document {
    /// `org-footnote-get-definition`: the start of the first definition of
    /// `label`, either a `[fn:LABEL]` definition at the start of a line or
    /// an inline `[fn:LABEL: ...]` (labels compare case-insensitively).
    pub fn footnote_definition(&self, label: &str) -> Option<usize> {
        let label = label.strip_prefix("fn:").unwrap_or(label);
        let root = self.parse.syntax();
        let text = root.to_string();
        let mut candidates: Vec<(usize, usize)> = Vec::new();
        for n in root.descendants() {
            let start = usize::from(n.text_range().start());
            if let Some(d) = FootnoteDefinition::cast(n.clone()) {
                if d.label().eq_ignore_ascii_case(label)
                    || d.label().to_lowercase() == label.to_lowercase()
                {
                    // The `[fn:` must start the line.
                    let at = text[start..].find("[fn:").map(|i| start + i);
                    if let Some(p) = at
                        && (p == 0 || text.as_bytes()[p - 1] == b'\n')
                    {
                        candidates.push((p, start));
                    }
                }
            } else if let Some(r) = FootnoteReference::cast(n)
                && r.is_inline()
                && r.label()
                    .is_some_and(|l| l.to_lowercase() == label.to_lowercase())
                && start > 0
                && text.as_bytes()[start - 1] != b'\n'
            {
                candidates.push((start, start));
            }
        }
        candidates.into_iter().min().map(|(_, b)| b)
    }

    /// Every footnote reference with its definition.
    pub fn footnote_references(&self) -> Vec<FootnoteRef> {
        self.parse
            .syntax()
            .descendants()
            .filter_map(FootnoteReference::cast)
            .map(|r| {
                let label = r.label();
                FootnoteRef {
                    begin: usize::from(r.syntax().text_range().start()),
                    definition: label.as_deref().and_then(|l| self.footnote_definition(l)),
                    inline: r.is_inline(),
                    label,
                }
            })
            .collect()
    }
}
