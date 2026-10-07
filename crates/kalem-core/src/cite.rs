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
    load(&doc.bibliography(dir))
}

/// The entries of the bibliography `files`, read again only when one of
/// them changes.
pub fn load(files: &[PathBuf]) -> Arc<Bibliography> {
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
        let (bib, _errors) = Bibliography::load(files);
        let bib = Arc::new(bib);
        // A few documents' worth.
        if cache.len() > 16 {
            cache.clear();
        }
        cache.insert(stamp, bib.clone());
        return bib;
    }
    Arc::new(Bibliography::load(files).0)
}

/// A field as it prints: without the braces BibTeX protects words with,
/// its accents and named letters as characters (`B\"uy\"uk` is
/// `Büyük`), as the `.bib` grid shows it.
fn field<'a>(e: &'a Entry, name: &str) -> Option<std::borrow::Cow<'a, str>> {
    let v = e.field(name)?;
    Some(if v.contains(['{', '}', '\\', '~']) {
        std::borrow::Cow::Owned(crate::bibtex::plain(v))
    } else {
        std::borrow::Cow::Borrowed(v)
    })
}

/// The year of an entry: `year`, or the start of `date`.
pub fn year(e: &Entry) -> Option<String> {
    e.field("year").map(str::to_string).or_else(|| {
        let d = e.field("date")?;
        (d.len() >= 4 && d.as_bytes()[..4].iter().all(u8::is_ascii_digit))
            .then(|| d[..4].to_string())
    })
}

/// The family names of an entry's authors (or editors), as a citation
/// names them: `Knuth`, `Knuth and Lamport`, `Knuth et al.`.
pub fn short_authors(e: &Entry) -> Option<String> {
    let who = field(e, "author").or_else(|| field(e, "editor"))?;
    Some(crate::bibstyle::surnames(&who)).filter(|s| !s.is_empty())
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
/// `text` is the text `doc` was parsed from.
pub fn note_at(doc: &Document, text: &str, file: Option<&Path>, pos: usize) -> Option<String> {
    preview(doc, file, pos).or_else(|| {
        let root = doc.parse().syntax();
        let (label, body) = org_edit::footnote::preview_in(text, &root, pos)?;
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
    at: Option<(Option<PathBuf>, u64, usize, u64)>,
    text: Option<String>,
}

impl Preview {
    /// The preview at `doc`'s cursor.
    pub fn get(&mut self, doc: &mut crate::document::DocumentState) -> Option<String> {
        let head = doc.selection.head;
        // LaTeX diagnostics arrive after the text changed.
        let generation = doc.latex().map_or(0, |l| l.diagnostics.generation());
        let at = (doc.meta.path.clone(), doc.version(), head, generation);
        if self.at.as_ref() != Some(&at) {
            let path = at.0.clone();
            self.at = Some(at);
            // The model waits for the parse: it is of the text as it is.
            self.text = doc
                .model()
                .and_then(|m| note_at(&m, doc.text().as_str(), path.as_deref(), head))
                .or_else(|| crate::latex_view::note_at(doc, head))
                .or_else(|| crate::latex_view::diagnostic_at(doc, head));
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
            "@book{knuth84, author = {Donald E. Knuth}, title = {The {\\TeX}book}, publisher = {Addison-Wesley}, year = 1984}\n@book{b, author = {B\\\"uy\\\"uk, Ay\\c{s}e}, title = {\\\"Ozet}, year = 2020}\n",
        )
        .unwrap();
        let file = dir.join("doc.org");
        let d = doc("#+bibliography: refs.bib\n\nAs [cite:@knuth84] and [cite:@nope].\n");
        let bib = bibliography(&d, Some(&file));
        let items = picker_items(&bib);
        assert_eq!(items.len(), 2);
        // As it prints: no braces, no backslashes, accents as letters.
        assert_eq!(
            items[0].title,
            "@knuth84  Donald E. Knuth (1984). The TeXbook. Addison-Wesley."
        );
        assert_eq!(
            items[1].title,
            "@b  B\u{fc}y\u{fc}k, Ay\u{15f}e (2020). \u{d6}zet."
        );
        assert_eq!(
            crate::palette::split_invocation(&items[0].id),
            (INSERT, serde_json::json!({ "key": "knuth84" }))
        );
        assert_eq!(
            preview(&d, Some(&file), 38).as_deref(),
            Some("@knuth84: Donald E. Knuth (1984). The TeXbook. Addison-Wesley.")
        );
        let p = preview(&d, Some(&file), 57).unwrap();
        assert!(p.contains("nope"), "{p}");
        assert_eq!(preview(&d, Some(&file), 2), None);
    }
}

/// The CSL style for a `\bibliographystyle`: the one Kalem ships closest
/// to it, else the default.
pub fn csl_style(bibliography_style: Option<&str>) -> &'static str {
    match bibliography_style
        .map(|s| s.trim().to_ascii_lowercase())
        .as_deref()
    {
        Some(
            "ieeetr" | "ieee" | "ieeetran" | "unsrt" | "numeric" | "numeric-comp" | "plain"
            | "abbrv" | "alpha",
        ) => "ieee",
        Some("apalike" | "apa" | "apacite" | "plainnat" | "abbrvnat" | "unsrtnat") => "apa",
        Some("mla") => "modern-language-association",
        _ => org_cite::csl::DEFAULT_STYLE,
    }
}

/// Bibliography files with their modification times.
type Stamp = Vec<(PathBuf, Option<SystemTime>)>;

thread_local! {
    /// CSL styles loaded, by name.
    static STYLES: std::cell::RefCell<HashMap<String, Option<Arc<org_cite::csl::Processor>>>> =
        std::cell::RefCell::new(HashMap::new());
    /// The CSL library of the bibliography files last asked for.
    static LIBRARY: std::cell::RefCell<Option<(Stamp, Arc<org_cite::csl::Library>)>> =
        const { std::cell::RefCell::new(None) };
}

/// The entry `key` of the bibliography `files` as the bibliography of
/// the CSL `style` shows it, in plain text.
pub fn card(files: &[PathBuf], key: &str, style: &str) -> Option<String> {
    let processor = STYLES.with(|s| {
        s.borrow_mut()
            .entry(style.to_string())
            .or_insert_with(|| {
                org_cite::csl::Processor::new(Some(style), None, None)
                    .ok()
                    .map(Arc::new)
            })
            .clone()
    })?;
    let stamp: Vec<(PathBuf, Option<SystemTime>)> = files
        .iter()
        .map(|f| {
            (
                f.clone(),
                std::fs::metadata(f).and_then(|m| m.modified()).ok(),
            )
        })
        .collect();
    let lib = LIBRARY.with(|l| {
        let mut l = l.borrow_mut();
        match &*l {
            Some((s, lib)) if *s == stamp => lib.clone(),
            _ => {
                let lib = Arc::new(org_cite::csl::Library::load(files).0);
                *l = Some((stamp, lib.clone()));
                lib
            }
        }
    });
    lib.get(key)?;
    let r = processor.render(
        &lib,
        &[org_cite::csl::CiteRequest {
            items: vec![org_cite::csl::ItemRequest {
                key: key.to_string(),
                locator: None,
                mode: org_cite::csl::Mode::Normal,
            }],
            hidden: true,
            note_number: None,
        }],
    );
    let (_, label, spans) = r.bibliography?.items.into_iter().next()?;
    let text = org_cite::csl::plain(&spans);
    let text = match label {
        Some(l) => format!("{} {}", org_cite::csl::plain(&l), text.trim()),
        None => text.trim().to_string(),
    };
    (!text.is_empty()).then_some(text)
}

/// BibTeX pasted into a LaTeX document: its entries added to the
/// document's first bibliography file (those it has already left as they
/// are), and a `\cite` of them to insert instead.
pub fn pasted_bibtex(doc: &crate::DocumentState, text: &str) -> Option<String> {
    if !text.trim_start().starts_with('@') {
        return None;
    }
    let entries = org_cite::bib::parse_bibtex(text).ok()?;
    if entries.is_empty() {
        return None;
    }
    // The first bibliography file, found as LaTeX finds it (from the root
    // document's folder).
    let file = doc
        .latex()?
        .bibliography_files(doc.meta.path.as_deref())
        .into_iter()
        .next()?;
    // Read as the editors read it, its encoding kept; a file there that
    // cannot be read is left alone, never replaced.
    let (old, meta) = match crate::files::read(&file) {
        Ok((t, m, _)) => (t, Some(m)),
        Err(_) if file.exists() => return None,
        Err(_) => (String::new(), None),
    };
    // Its keys as the bib mode reads them: an entry with a mistake does
    // not hide the others.
    let known: std::collections::HashSet<&str> = crate::bibtex::entries(&old)
        .iter()
        .map(|e| &old[e.key.clone()])
        .collect();
    // Each new entry's own text, as the bib mode cuts it (an `@` in a
    // value is not an entry).
    let mut add = String::new();
    let mut seen = std::collections::HashSet::new();
    for e in crate::bibtex::entries(text) {
        let key = &text[e.key.clone()];
        if key.is_empty() || known.contains(key) || !seen.insert(key) {
            continue;
        }
        add.push('\n');
        add.push_str(text[e.range.clone()].trim());
        add.push('\n');
    }
    if !add.is_empty() {
        let mut new = old.clone();
        if !new.is_empty() && !new.ends_with('\n') {
            new.push('\n');
        }
        if new.is_empty() {
            add.remove(0);
        }
        new.push_str(&add);
        let bytes = match &meta {
            Some(m) => crate::files::encode(&new, m),
            None => new.into_bytes(),
        };
        std::fs::write(&file, bytes).ok()?;
    }
    let keys: Vec<String> = entries.into_iter().map(|e| e.key).collect();
    Some(format!("\\cite{{{}}}", keys.join(",")))
}

#[cfg(test)]
mod card_tests {
    #[test]
    fn cards_in_a_csl_style() {
        let dir = std::env::temp_dir().join(format!("kalem-card-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let f = dir.join("refs.bib");
        std::fs::write(
            &f,
            "@book{knuth84, author = {Donald E. Knuth}, title = {The {\\TeX}book}, publisher = {Addison-Wesley}, year = {1984}}\n",
        )
        .unwrap();
        let files = vec![f];
        let apa = super::card(&files, "knuth84", "apa").unwrap();
        assert!(apa.starts_with("Knuth, D. E. (1984)"), "{apa}");
        let ieee = super::card(&files, "knuth84", super::csl_style(Some("ieeetr"))).unwrap();
        assert!(ieee.starts_with("[1]"), "{ieee}");
        assert_eq!(super::card(&files, "nope", "apa"), None);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn bibtex_pasted_into_latex() {
        let dir = std::env::temp_dir().join(format!("kalem-paste-bib-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let bib = dir.join("refs.bib");
        std::fs::write(&bib, "@book{old, title = {O}}").unwrap();
        let text = "See \n\\bibliography{refs}\n";
        let meta = crate::Metadata {
            path: Some(dir.join("p.tex")),
            mode: crate::DocumentMode::Latex,
            line_ending: crate::LineEnding::Lf,
            bom: false,
            encoding: encoding_rs::UTF_8,
            lossy: false,
        };
        let mut d = crate::DocumentState::new(
            text,
            meta,
            std::sync::Arc::new(org_model::Settings::default()),
        );
        d.selection = org_edit::Selection::caret(4);
        let now = std::time::Instant::now();
        d.paste(
            "@article{new1,\n  title = {N},\n  year = 2020\n}\n@book{old, title = {O}}\n",
            None,
            false,
            now,
        );
        assert_eq!(
            d.text().as_str(),
            "See \\cite{new1,old}\n\\bibliography{refs}\n"
        );
        assert_eq!(
            std::fs::read_to_string(&bib).unwrap(),
            "@book{old, title = {O}}\n\n@article{new1,\n  title = {N},\n  year = 2020\n}\n"
        );
        // Plain text stays text.
        d.paste("@someone", None, false, now);
        assert!(d.text().as_str().contains("@someone"));
        // An entry of the file with a mistake hides no key, and an `@` in
        // a value starts no entry.
        std::fs::write(&bib, "@book{old, title = {O}}\n@misc{bad title = {B}}\n").unwrap();
        d.paste(
            "@book{old, title = {O}}\n@misc{new2, note = {Follow @kalem on the web}, title = {N}}\n",
            None,
            false,
            now,
        );
        assert_eq!(
            std::fs::read_to_string(&bib).unwrap(),
            "@book{old, title = {O}}\n@misc{bad title = {B}}\n\n@misc{new2, note = {Follow @kalem on the web}, title = {N}}\n"
        );
        // A file in Latin-1 keeps its text and its encoding.
        std::fs::write(&bib, b"@book{old, author = {G\xf6del}}\n").unwrap();
        d.paste("@misc{new3, title = {N}}\n", None, false, now);
        assert_eq!(
            std::fs::read(&bib).unwrap(),
            b"@book{old, author = {G\xf6del}}\n\n@misc{new3, title = {N}}\n"
        );
        std::fs::remove_dir_all(&dir).ok();
    }
}
