//! Language packs (§11.11, T2.7a.7): formats whose view is their source
//! (JSON, YAML, diff, ledger, `.po`…) get no document mode, only four
//! hooks on top of their highlighting, the same a plugin gets:
//!
//! - an outline provider, for the outline sidebar of both editors;
//! - a formatter, for Format Document and `kalem fmt`;
//! - a completer, on the completer contract ([`crate::completers`]);
//! - diagnostics, for the status bar and `kalem check`.
//!
//! A pack serves text types (the highlighter's language names, the
//! `textType` of when-clauses). The packs themselves live in
//! `getkalem/plugins` (D29) and register here through the plugin host;
//! the core carries the contract, its wiring and its tests.

use std::sync::{Arc, RwLock};

use crate::DocumentState;
use crate::modes::ModeDiagnostic;
use crate::view::OutlineItem;

/// What a formatter says about a text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Formatted {
    /// The text in its formatted form (the same text when it already is).
    Text(String),
    /// The text cannot be formatted as it is (a syntax error): why and
    /// where.
    Refused(ModeDiagnostic),
}

/// A language pack. Only `id` and `languages` are required; a hook a pack
/// does not give is simply absent.
pub trait LanguagePack: Send + Sync {
    /// Its name: `json`, `diff`.
    fn id(&self) -> &'static str;
    /// The text types it serves.
    fn languages(&self) -> &[&str];
    /// The outline: headings for the sidebar, in text order.
    fn outline(&self, _text: &str) -> Option<Vec<OutlineItem>> {
        None
    }
    /// The formatted text, `None` without a formatter.
    fn format(&self, _text: &str) -> Option<Formatted> {
        None
    }
    /// Problems in the text, in text order.
    fn diagnostics(&self, _text: &str) -> Vec<ModeDiagnostic> {
        Vec::new()
    }
    /// Its completer, scoped to its languages.
    fn completer(&self) -> Option<Arc<dyn crate::completers::Completer>> {
        None
    }
}

/// The packs there are.
static PACKS: RwLock<Vec<Arc<dyn LanguagePack>>> = RwLock::new(Vec::new());

/// Adds a pack (a plugin's); a later pack for the same language comes
/// first.
pub fn register(pack: Arc<dyn LanguagePack>) {
    if let Ok(mut p) = PACKS.write() {
        p.retain(|q| q.id() != pack.id());
        p.insert(0, pack);
    }
}

/// Removes the pack `id` (a plugin disabled).
pub fn unregister(id: &str) {
    if let Ok(mut p) = PACKS.write() {
        p.retain(|q| q.id() != id);
    }
}

/// Every pack, the latest first.
pub fn all() -> Vec<Arc<dyn LanguagePack>> {
    PACKS.read().map(|p| p.clone()).unwrap_or_default()
}

/// The pack serving `language`.
pub fn for_language(language: &str) -> Option<Arc<dyn LanguagePack>> {
    let language = crate::command::canonical_type(language);
    all().into_iter().find(|p| {
        p.languages()
            .iter()
            .any(|l| crate::command::canonical_type(l) == language)
    })
}

/// The language of a text document, as packs see it.
fn language_of(doc: &DocumentState) -> Option<String> {
    match &doc.meta.mode {
        crate::DocumentMode::Text { language: Some(l) } => Some(crate::command::canonical_type(l)),
        _ => None,
    }
}

/// The pack of a text document.
pub fn for_document(doc: &DocumentState) -> Option<Arc<dyn LanguagePack>> {
    for_language(&language_of(doc)?)
}

/// The outline of a text document from its pack.
pub fn outline_items(doc: &DocumentState) -> Option<Vec<OutlineItem>> {
    for_document(doc)?.outline(doc.text().as_str())
}

/// Whether the document's pack formats it (for Format Document's
/// when-clause, `hasFormatter`).
pub fn has_formatter(doc: &DocumentState) -> bool {
    for_document(doc).is_some_and(|p| p.format("").is_some())
}

/// The formatted text of a text document from its pack.
pub fn format(doc: &DocumentState) -> Option<Formatted> {
    for_document(doc)?.format(doc.text().as_str())
}

/// The diagnostics of a text document from its pack.
pub fn diagnostics(doc: &DocumentState) -> Vec<ModeDiagnostic> {
    for_document(doc).map_or_else(Vec::new, |p| p.diagnostics(doc.text().as_str()))
}

/// What the status bar says about a text document's problems: the one on
/// the cursor's line, else how many there are.
pub fn status(doc: &DocumentState) -> Option<String> {
    let diags = diagnostics(doc);
    if diags.is_empty() {
        return None;
    }
    let text = doc.text();
    let line = text.line_of(doc.selection.head.min(text.len()));
    if let Some(d) = diags
        .iter()
        .find(|d| text.line_of(d.range.start.min(text.len())) == line)
    {
        return Some(d.message.clone());
    }
    Some(crate::tr!("status-problems", count = diags.len()))
}

/// The completers of the packs, for the completer registry.
pub fn completers() -> Vec<Arc<dyn crate::completers::Completer>> {
    all().iter().filter_map(|p| p.completer()).collect()
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// A pack for tests: `key = value` lines, sections in brackets as the
    /// outline, a formatter that writes ` = ` with single spaces, and a
    /// diagnostic for a line without `=`.
    pub(crate) struct IniPack;

    impl LanguagePack for IniPack {
        fn id(&self) -> &'static str {
            "test-ini"
        }
        fn languages(&self) -> &[&str] {
            &["kalem-test-ini"]
        }
        fn outline(&self, text: &str) -> Option<Vec<OutlineItem>> {
            let mut at = 0;
            let mut out = Vec::new();
            for line in text.split_inclusive('\n') {
                let t = line.trim_end();
                if let Some(name) = t.strip_prefix('[').and_then(|s| s.strip_suffix(']')) {
                    out.push(OutlineItem {
                        level: 1,
                        todo: None,
                        title: name.to_string(),
                        start: at,
                    });
                }
                at += line.len();
            }
            Some(out)
        }
        fn format(&self, text: &str) -> Option<Formatted> {
            if let Some(d) = self.diagnostics(text).into_iter().next() {
                return Some(Formatted::Refused(d));
            }
            let lines: Vec<String> = text
                .lines()
                .map(|l| match l.split_once('=') {
                    Some((k, v)) if !l.starts_with('[') => format!("{} = {}", k.trim(), v.trim()),
                    _ => l.to_string(),
                })
                .collect();
            let mut s = lines.join("\n");
            if text.ends_with('\n') {
                s.push('\n');
            }
            Some(Formatted::Text(s))
        }
        fn diagnostics(&self, text: &str) -> Vec<ModeDiagnostic> {
            let mut at = 0;
            let mut out = Vec::new();
            for line in text.split_inclusive('\n') {
                let t = line.trim();
                if !t.is_empty() && !t.starts_with('[') && !t.contains('=') {
                    out.push(ModeDiagnostic {
                        range: at..at + line.trim_end().len(),
                        code: "no-value".into(),
                        message: format!("No value: {t}"),
                    });
                }
                at += line.len();
            }
            out
        }
    }

    #[test]
    fn a_pack_serves_its_language() {
        register(Arc::new(IniPack));
        let p = for_language("kalem-test-ini").unwrap();
        assert_eq!(p.id(), "test-ini");
        assert!(for_language("kalem-test-none").is_none());
        let t = "[a]\nx=1\n[b]\ny =  2\n";
        let o = p.outline(t).unwrap();
        assert_eq!(
            o.iter()
                .map(|i| (i.title.as_str(), i.start))
                .collect::<Vec<_>>(),
            [("a", 0), ("b", 8)]
        );
        assert_eq!(
            p.format(t),
            Some(Formatted::Text("[a]\nx = 1\n[b]\ny = 2\n".into()))
        );
        assert!(matches!(p.format("oops\n"), Some(Formatted::Refused(_))));
        assert_eq!(p.diagnostics("[a]\noops\n")[0].range, 4..8);
    }
}
