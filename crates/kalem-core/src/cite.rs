//! Citations in the editor: the citation picker (the entries of the
//! document's bibliography, searched by key, author and title), inserting
//! a citation, and the preview of the entry cited under the cursor.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::SystemTime;

use org_cite::{Bibliography, Entry};
use org_edit::{EditError, Selection, Transaction};
use org_model::Document;
use org_syntax::SyntaxKind;
use org_syntax::ast::{AstNode, CitationReference};

use crate::palette::PaletteItem;

/// The command that inserts a citation; with a `key` it inserts that key,
/// without it shows the picker.
pub const INSERT: &str = "org.cite.insert";

/// The bibliographies read, by their files and their modification times.
type Cache = HashMap<Vec<(PathBuf, Option<SystemTime>)>, Arc<Bibliography>>;

static CACHE: Mutex<Option<Cache>> = Mutex::new(None);

/// The entries of `doc`'s `#+BIBLIOGRAPHY:` files (relative to `file`'s
/// folder), read again only when a file changes.
pub fn bibliography(doc: &Document, file: Option<&Path>) -> Arc<Bibliography> {
    let dir = file.and_then(Path::parent);
    let files = doc.bibliography(dir);
    let stamp: Vec<(PathBuf, Option<SystemTime>)> = files
        .iter()
        .map(|f| {
            let t = std::fs::metadata(f).and_then(|m| m.modified()).ok();
            (f.clone(), t)
        })
        .collect();
    if let Ok(mut c) = CACHE.lock() {
        let cache = c.get_or_insert_with(HashMap::new);
        if let Some(b) = cache.get(&stamp) {
            return b.clone();
        }
        let (bib, _errors) = Bibliography::load(&files);
        let bib = Arc::new(bib);
        // A few documents' worth.
        if cache.len() > 16 {
            cache.clear();
        }
        cache.insert(stamp, bib.clone());
        return bib;
    }
    Arc::new(Bibliography::load(&files).0)
}

/// A field without the braces BibTeX protects words with.
fn field<'a>(e: &'a Entry, name: &str) -> Option<std::borrow::Cow<'a, str>> {
    let v = e.field(name)?;
    Some(if v.contains(['{', '}']) {
        std::borrow::Cow::Owned(v.replace(['{', '}'], ""))
    } else {
        std::borrow::Cow::Borrowed(v)
    })
}

/// The year of an entry: `year`, or the start of `date`.
fn year(e: &Entry) -> Option<String> {
    e.field("year").map(str::to_string).or_else(|| {
        let d = e.field("date")?;
        (d.len() >= 4 && d.as_bytes()[..4].iter().all(u8::is_ascii_digit))
            .then(|| d[..4].to_string())
    })
}

/// One line about an entry: "Author (Year). Title. Journal."
pub fn describe(e: &Entry) -> String {
    let mut out = String::new();
    let who = field(e, "author").or_else(|| field(e, "editor"));
    if let Some(a) = &who {
        out.push_str(a);
    }
    if let Some(y) = year(e) {
        if !out.is_empty() {
            out.push(' ');
        }
        out.push_str(&format!("({y})"));
    }
    if let Some(t) = field(e, "title") {
        if !out.is_empty() {
            out.push_str(". ");
        }
        out.push_str(t.trim_end_matches('.'));
    }
    let from = ["journal", "booktitle", "publisher", "institution", "school"]
        .iter()
        .find_map(|f| field(e, f));
    if let Some(f) = from {
        out.push_str(". ");
        out.push_str(&f);
    }
    if !out.is_empty() && !out.ends_with('.') {
        out.push('.');
    }
    out
}

/// The picker's items: each entry, found by its key, authors and title;
/// choosing one runs [`INSERT`] with its key.
pub fn picker_items(bib: &Bibliography) -> Vec<PaletteItem> {
    bib.entries()
        .iter()
        .map(|e| PaletteItem {
            id: crate::palette::invocation(INSERT, &serde_json::json!({ "key": e.key })),
            title: format!("@{}  {}", e.key, describe(e)),
            category: e.kind.to_lowercase(),
            keys: String::new(),
            also: String::new(),
        })
        .collect()
}

/// The citation reference at `pos`: its key.
pub fn key_at(doc: &Document, pos: usize) -> Option<String> {
    let root = doc.parse().syntax();
    let offset = org_syntax::TextSize::try_from(pos).ok()?;
    let token = root.token_at_offset(offset).right_biased()?;
    let reference = token
        .parent_ancestors()
        .find(|n| n.kind() == SyntaxKind::CITATION_REFERENCE)
        .and_then(CitationReference::cast)?;
    Some(reference.key())
}

/// What the status bar says about the citation under the cursor: the
/// entry it cites, or that the bibliography does not have it.
pub fn preview(doc: &Document, file: Option<&Path>, pos: usize) -> Option<String> {
    let key = key_at(doc, pos)?;
    let bib = bibliography(doc, file);
    Some(match bib.get(&key) {
        Some(e) => format!("@{key}: {}", describe(e)),
        None => crate::tr!("cite-unknown-key", key = key.as_str()),
    })
}

/// What the status bar and a tooltip say about the citation or the
/// footnote reference at `pos`: the entry cited, or the footnote's text.
pub fn note_at(doc: &Document, file: Option<&Path>, pos: usize) -> Option<String> {
    preview(doc, file, pos).or_else(|| {
        let text = doc.parse().syntax().to_string();
        let (label, body) = org_edit::footnote::preview(&text, pos)?;
        Some(crate::tr!(
            "footnote-preview",
            label = label.unwrap_or_default().as_str(),
            text = body.as_str()
        ))
    })
}

/// [`note_at`] for the cursor of a document, worked out again only when
/// the document or the cursor changes.
#[derive(Debug, Default)]
pub struct Preview {
    at: Option<(Option<PathBuf>, u64, usize)>,
    text: Option<String>,
}

impl Preview {
    /// The preview at `doc`'s cursor.
    pub fn get(&mut self, doc: &mut crate::document::DocumentState) -> Option<String> {
        let head = doc.selection.head;
        let at = (doc.meta.path.clone(), doc.version(), head);
        if self.at.as_ref() != Some(&at) {
            let path = at.0.clone();
            self.at = Some(at);
            self.text = doc.model().and_then(|m| note_at(&m, path.as_deref(), head));
        }
        self.text.clone()
    }
}

/// Inserts a citation of `key` at `point`: a new `[cite:@key]`, or, in a
/// citation, one more reference at its end.
pub fn insert(doc: &Document, point: usize, key: &str) -> Result<Transaction, EditError> {
    let root = doc.parse().syntax();
    let inside = root
        .descendants()
        .filter(|n| n.kind() == SyntaxKind::CITATION)
        .filter_map(|n| {
            // Up to its closing bracket (the blanks after it are part of
            // the node).
            let start = usize::from(n.text_range().start());
            let close = start + n.text().to_string().rfind(']')?;
            Some(start..close)
        })
        .find(|r| r.start < point && point <= r.end);
    let (at, text) = match inside {
        // Before the closing bracket.
        Some(r) => (r.end, format!("; @{key}")),
        None => (point, format!("[cite:@{key}]")),
    };
    let mut tx = Transaction::new("Insert Citation");
    tx.replace(at..at, &text).map_err(|_| EditError {
        message: "cannot insert here".into(),
        point: None,
    })?;
    Ok(tx.select(Selection::caret(at + text.len())))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc(text: &str) -> Document {
        Document::new(org_syntax::parse(text))
    }

    #[test]
    fn keys_and_insertion() {
        let d = doc("See [cite/t:see @knuth84 p. 3; @doe-2020] here.\n");
        assert_eq!(key_at(&d, 17).as_deref(), Some("knuth84"));
        assert_eq!(key_at(&d, 35).as_deref(), Some("doe-2020"));
        assert_eq!(key_at(&d, 2), None);
        let apply = |d: &Document, tx: Transaction| tx.apply(&d.parse().syntax().to_string());
        let tx = insert(&d, 20, "new").unwrap();
        assert_eq!(
            apply(&d, tx),
            "See [cite/t:see @knuth84 p. 3; @doe-2020; @new] here.\n"
        );
        let tx = insert(&d, 3, "new").unwrap();
        assert_eq!(
            apply(&d, tx),
            "See[cite:@new] [cite/t:see @knuth84 p. 3; @doe-2020] here.\n"
        );
    }

    #[test]
    fn picker_and_preview() {
        let dir = std::env::temp_dir().join(format!("kalem-cite-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("refs.bib"),
            "@book{knuth84, author = {Donald E. Knuth}, title = {The {\\TeX}book}, publisher = {Addison-Wesley}, year = 1984}\n",
        )
        .unwrap();
        let file = dir.join("doc.org");
        let d = doc("#+bibliography: refs.bib\n\nAs [cite:@knuth84] and [cite:@nope].\n");
        let bib = bibliography(&d, Some(&file));
        let items = picker_items(&bib);
        assert_eq!(items.len(), 1);
        assert_eq!(
            items[0].title,
            "@knuth84  Donald E. Knuth (1984). The \\TeXbook. Addison-Wesley."
        );
        assert_eq!(
            crate::palette::split_invocation(&items[0].id),
            (INSERT, serde_json::json!({ "key": "knuth84" }))
        );
        assert_eq!(
            preview(&d, Some(&file), 38).as_deref(),
            Some("@knuth84: Donald E. Knuth (1984). The \\TeXbook. Addison-Wesley.")
        );
        let p = preview(&d, Some(&file), 57).unwrap();
        assert!(p.contains("nope"), "{p}");
        assert_eq!(preview(&d, Some(&file), 2), None);
    }
}
