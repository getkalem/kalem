//! Two kinds of Org file (design §3.7, decision D24): a `.org` file is
//! strict Org, and Kalem never writes its own additions into it; a `.klm`
//! file is a Kalem document, Org with Kalem's additions written through
//! Org's extension points. A `.org` file opts in with `#+KALEM:
//! markup=yes`, or a folder with the setting `org.allow_kalem_markup`.

use std::path::{Path, PathBuf};

use crate::document::DocumentState;
use crate::mode::DocumentMode;

/// The file kind of an Org document: `org` for a `.org` (or
/// `.org_archive`) file, else `klm`, a Kalem document (an Org document not
/// saved yet is one until it is saved under another name); `None` for
/// other modes.
pub fn file_kind(doc: &DocumentState) -> Option<&'static str> {
    if doc.meta.mode != DocumentMode::Org {
        return None;
    }
    let org = doc
        .meta
        .path
        .as_deref()
        .and_then(Path::extension)
        .and_then(|e| e.to_str())
        .is_some_and(|e| {
            let e = e.to_ascii_lowercase();
            e == "org" || e == "org_archive"
        });
    Some(if org { "org" } else { "klm" })
}

/// Whether `path` names a Kalem document.
pub fn is_klm(path: Option<&Path>) -> bool {
    path.and_then(Path::extension)
        .is_some_and(|e| e.eq_ignore_ascii_case("klm"))
}

/// Whether Kalem's formatting may be written into `doc`: a Kalem document,
/// a `.org` file that opted in (`#+KALEM: markup=yes`), or any Org file
/// when `org.allow_kalem_markup` is on.
pub fn markup_allowed(doc: &DocumentState, config: &crate::settings::Config) -> bool {
    match file_kind(doc) {
        Some("klm") => true,
        Some(_) => {
            config.bool("org.allow_kalem_markup")
                || doc.parse().is_some_and(|(p, _)| {
                    crate::rich::kalem_option(&p.keywords(), "markup")
                        .is_some_and(|v| v.eq_ignore_ascii_case("yes"))
                })
        }
        None => false,
    }
}

/// What the list offered when Kalem's formatting is asked for in a strict
/// `.org` file holds: make it a Kalem document, or allow the formatting
/// in this file.
pub fn offer_items() -> Vec<crate::palette::PaletteItem> {
    use crate::l10n::tr;
    ["file.makeKalemDocument", "format.allowMarkup"]
        .into_iter()
        .map(|id| crate::palette::PaletteItem {
            id: id.to_string(),
            title: tr(&crate::l10n::command_key(id)),
            category: tr("kind-strict-org"),
            keys: String::new(),
            also: id.replace('.', " "),
        })
        .collect()
}

/// What Kalem adds to Org, found in a document.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Markup {
    /// A formatting snippet: `@@kalem:…@@`.
    Span,
    /// A paragraph attribute line: `#+ATTR_KALEM:`.
    Paragraph,
    /// The document's options: `#+KALEM:`.
    Document,
}

/// Kalem's additions in the document `root`, in order: each one's range
/// (whole lines for `#+ATTR_KALEM:` and `#+KALEM:`, with their line
/// feeds) and kind.
pub fn markup(root: &org_syntax::SyntaxNode) -> Vec<(std::ops::Range<usize>, Markup)> {
    use org_syntax::SyntaxKind::*;
    use org_syntax::ast::{self, AstNode};
    let range = |n: &org_syntax::SyntaxNode| {
        let r = n.text_range();
        usize::from(r.start())..usize::from(r.end())
    };
    let mut out = Vec::new();
    for n in root.descendants() {
        match n.kind() {
            EXPORT_SNIPPET => {
                if let Some(s) = ast::ExportSnippet::cast(n.clone())
                    && s.backend().eq_ignore_ascii_case("kalem")
                {
                    let r = range(&n);
                    let end = r.end - ast::post_blank(&n);
                    out.push((r.start..end, Markup::Span));
                }
            }
            KEYWORD => {
                if let Some(k) = ast::Keyword::cast(n.clone())
                    && k.key().eq_ignore_ascii_case("KALEM")
                {
                    out.push((range(&n), Markup::Document));
                }
            }
            _ => {
                for k in ast::affiliated_keywords(&n) {
                    if k.key().eq_ignore_ascii_case("ATTR_KALEM") {
                        out.push((range(k.syntax()), Markup::Paragraph));
                    }
                }
            }
        }
    }
    out.sort_by_key(|(r, _)| r.start);
    out.dedup();
    out
}

/// `text` as strict Org: Kalem's additions taken out (a snippet goes, a
/// line of its own goes with its line feed), and how many of each kind
/// went (spans, paragraph attributes, document options). Org's own center
/// blocks stay.
pub fn strip_markup(text: &str) -> (String, [usize; 3]) {
    let parse = org_syntax::parse(text);
    let found = markup(&parse.syntax());
    let mut counts = [0usize; 3];
    let mut out = String::with_capacity(text.len());
    let mut at = 0;
    for (r, kind) in found {
        if r.start < at {
            continue;
        }
        counts[kind as usize] += 1;
        out.push_str(&text[at..r.start]);
        at = r.end;
    }
    out.push_str(&text[at..]);
    (out, counts)
}

/// What [`strip_markup`] took out, for a message: `3 formatted spans,
/// 1 paragraph attribute`.
pub fn dropped_summary(counts: [usize; 3]) -> String {
    let parts: Vec<String> = [
        ("kind-dropped-spans", counts[0]),
        ("kind-dropped-paragraphs", counts[1]),
        ("kind-dropped-document", counts[2]),
    ]
    .into_iter()
    .filter(|(_, n)| *n > 0)
    .map(|(id, n)| crate::tr!(id, count = n))
    .collect();
    if parts.is_empty() {
        crate::l10n::tr("kind-dropped-nothing")
    } else {
        parts.join(", ")
    }
}

/// `to` relative to the folder `from`, with `/` between names: `b.org`,
/// `sub/b.org`, `../b.org`.
pub fn relative(from: &Path, to: &Path) -> Option<String> {
    let from: Vec<_> = from.components().collect();
    let to: Vec<_> = to.components().collect();
    let common = from.iter().zip(&to).take_while(|(a, b)| a == b).count();
    if common == 0 {
        return None;
    }
    let mut parts: Vec<String> = vec!["..".to_string(); from.len() - common];
    parts.extend(
        to[common..]
            .iter()
            .map(|c| c.as_os_str().to_string_lossy().into_owned()),
    );
    Some(parts.join("/"))
}

/// Replaces links to `old` with links to `new` in the Org files (`.org`
/// and `.klm`) under `root`, skipping what version control ignores: the
/// forms `[[file:PATH`, `[[./PATH` and `[[PATH` with `PATH` relative to
/// each file, or absolute. Returns the files changed.
pub fn update_links(root: &Path, old: &Path, new: &Path) -> Vec<PathBuf> {
    let cancel = std::sync::atomic::AtomicBool::new(false);
    let mut files = Vec::new();
    kalem_project::walk(root, &[], &cancel, |rel| files.push(root.join(rel)));
    let mut changed = Vec::new();
    for f in files {
        let org = f
            .extension()
            .and_then(|e| e.to_str())
            .is_some_and(|e| matches!(e.to_ascii_lowercase().as_str(), "org" | "klm"));
        if !org || f == new {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(&f) else {
            continue;
        };
        let Some(dir) = f.parent() else { continue };
        let mut pairs: Vec<(String, String)> = Vec::new();
        if let (Some(o), Some(n)) = (relative(dir, old), relative(dir, new)) {
            for prefix in ["[[file:", "[[./", "[[file:./"] {
                let o2 = o.trim_start_matches("./");
                pairs.push((
                    format!("{prefix}{o2}"),
                    format!("{prefix}{}", n.trim_start_matches("./")),
                ));
            }
            if o.starts_with("..") {
                pairs.push((format!("[[{o}"), format!("[[{n}")));
            }
        }
        pairs.push((
            format!("[[file:{}", old.display()),
            format!("[[file:{}", new.display()),
        ));
        let mut out = text.clone();
        for (a, b) in &pairs {
            // Only whole names: the link's path ends at `]` or `::`.
            for end in ["]", "::"] {
                out = out.replace(&format!("{a}{end}"), &format!("{b}{end}"));
            }
        }
        if out != text && std::fs::write(&f, out).is_ok() {
            changed.push(f);
        }
    }
    changed
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stripping_kalem_markup() {
        let text = "#+TITLE: T\n#+KALEM: font=\"Georgia\" size=12\nSome @@kalem:color=red@@red@@kalem:end@@ text.\n\n#+ATTR_KALEM: :align right\n#+CAPTION: kept\nRight.\n\n#+begin_center\nMiddle\n#+end_center\n";
        let (out, counts) = strip_markup(text);
        assert_eq!(
            out,
            "#+TITLE: T\nSome red text.\n\n#+CAPTION: kept\nRight.\n\n#+begin_center\nMiddle\n#+end_center\n"
        );
        assert_eq!(counts, [2, 1, 1]);
        crate::l10n::set_language("en");
        assert_eq!(
            dropped_summary(counts),
            "2 formatted spans, 1 paragraph attribute, 1 document option line"
        );
        assert_eq!(strip_markup("* A\n").0, "* A\n");
    }

    #[test]
    fn kinds_and_relative_paths() {
        assert!(is_klm(Some(Path::new("/a/notes.KLM"))));
        assert!(!is_klm(Some(Path::new("/a/notes.org"))));
        assert_eq!(
            relative(Path::new("/p/a"), Path::new("/p/a/b.org")).as_deref(),
            Some("b.org")
        );
        assert_eq!(
            relative(Path::new("/p/a/c"), Path::new("/p/a/b.org")).as_deref(),
            Some("../b.org")
        );
        assert_eq!(
            relative(Path::new("/p"), Path::new("/p/a/b.org")).as_deref(),
            Some("a/b.org")
        );
    }

    #[test]
    fn links_follow_the_new_name() {
        let d = std::env::temp_dir().join(format!("kalem-kinds-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join("sub")).unwrap();
        let d = d.canonicalize().unwrap();
        std::fs::write(
            d.join("index.org"),
            "[[file:notes.org][Notes]] [[file:notes.org::*A]] [[./notes.org]] [[file:notes.orgx]]\n",
        )
        .unwrap();
        std::fs::write(
            d.join("sub/deep.klm"),
            "[[../notes.org]] [[file:../notes.org]]\n",
        )
        .unwrap();
        std::fs::write(d.join("readme.txt"), "[[file:notes.org]]\n").unwrap();
        let changed = update_links(&d, &d.join("notes.org"), &d.join("notes.klm"));
        assert_eq!(changed.len(), 2);
        assert_eq!(
            std::fs::read_to_string(d.join("index.org")).unwrap(),
            "[[file:notes.klm][Notes]] [[file:notes.klm::*A]] [[./notes.klm]] [[file:notes.orgx]]\n"
        );
        assert_eq!(
            std::fs::read_to_string(d.join("sub/deep.klm")).unwrap(),
            "[[../notes.klm]] [[file:../notes.klm]]\n"
        );
        assert_eq!(
            std::fs::read_to_string(d.join("readme.txt")).unwrap(),
            "[[file:notes.org]]\n"
        );
    }
}
