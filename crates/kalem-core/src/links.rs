//! Stored links (`org-store-link` and `org-insert-link`, T2.7e.10): the
//! files of the file manager, or a document and its heading, kept to be
//! inserted as `[[file:…]]` links, relative when the file is in the
//! document's folder tree.

use std::cell::RefCell;
use std::path::{Path, PathBuf};

/// A link kept to insert later.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Stored {
    /// The file or folder, absolute.
    pub path: PathBuf,
    /// A search option (`*Heading`).
    pub search: Option<String>,
    /// What the link shows.
    pub description: String,
}

thread_local! {
    /// The links stored, by store: the last store's links at the end (on
    /// the frontend's thread).
    static STORED: RefCell<Vec<Vec<Stored>>> = const { RefCell::new(Vec::new()) };
}

/// Keeps `links` as the latest stored.
pub fn store(links: Vec<Stored>) {
    if links.is_empty() {
        return;
    }
    STORED.with(|s| {
        let mut s = s.borrow_mut();
        s.retain(|b| *b != links);
        s.push(links);
        if s.len() > 50 {
            s.remove(0);
        }
    });
}

/// The links of the latest store.
pub fn latest() -> Vec<Stored> {
    STORED.with(|s| s.borrow().last().cloned().unwrap_or_default())
}

/// The `file:` target of `path` from a document in `dir`: relative when
/// the file is in `dir` or below it (`org-link-file-path-type`
/// `adaptive`), absolute otherwise; `/` separators.
pub fn file_target(path: &Path, dir: Option<&Path>, search: Option<&str>) -> String {
    let rel = dir.and_then(|d| path.strip_prefix(d).ok());
    let mut t = match rel {
        Some(r) if !r.as_os_str().is_empty() => {
            let s = r.to_string_lossy().replace('\\', "/");
            if s.starts_with('.') || s.contains(':') {
                format!("./{s}")
            } else {
                s
            }
        }
        _ => path.to_string_lossy().replace('\\', "/"),
    };
    if let Some(s) = search {
        t.push_str("::");
        t.push_str(s);
    }
    format!("file:{t}")
}

/// What `doc` would store: the file manager's marked files (or the one at
/// the cursor), else the document's file with the heading at the cursor.
pub fn links_of(doc: &mut crate::DocumentState) -> Vec<Stored> {
    let name = |p: &Path| {
        p.file_name().map_or_else(
            || p.display().to_string(),
            |n| n.to_string_lossy().into_owned(),
        )
    };
    if let Some(s) = doc.dired.as_deref() {
        let line = doc.text().line_of(doc.selection.head);
        return s
            .targets(line)
            .into_iter()
            .map(|p| Stored {
                description: name(&p),
                path: p,
                search: None,
            })
            .collect();
    }
    let Some(path) = doc.meta.path.clone() else {
        return Vec::new();
    };
    let path = std::path::absolute(&path).unwrap_or(path);
    let pos = doc.selection.head;
    let heading = doc.model().and_then(|m| {
        let h = crate::properties::heading(&m, pos)?;
        let title = h.title()?.text().to_string();
        let title = title.trim().to_string();
        (!title.is_empty()).then_some(title)
    });
    vec![Stored {
        description: heading.clone().unwrap_or_else(|| name(&path)),
        search: heading.map(|h| format!("*{h}")),
        path,
    }]
}

/// The Org text of `links` for a document at `doc_path`: one link, or one
/// a line.
pub fn org_text(links: &[Stored], doc_path: Option<&Path>) -> String {
    let dir = doc_path
        .map(|p| std::path::absolute(p).unwrap_or_else(|_| p.to_path_buf()))
        .and_then(|p| p.parent().map(Path::to_path_buf));
    links
        .iter()
        .filter_map(|l| {
            let path = std::path::absolute(&l.path).unwrap_or_else(|_| l.path.clone());
            let target = file_target(&path, dir.as_deref(), l.search.as_deref());
            org_edit::insert::link_string(&target, Some(&l.description)).ok()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn targets() {
        let dir = Path::new("/w/notes");
        assert_eq!(
            file_target(Path::new("/w/notes/sub/a.org"), Some(dir), None),
            "file:sub/a.org"
        );
        assert_eq!(
            file_target(Path::new("/w/notes/.hidden"), Some(dir), None),
            "file:./.hidden"
        );
        assert_eq!(
            file_target(Path::new("/w/other/b.txt"), Some(dir), Some("*Top")),
            "file:/w/other/b.txt::*Top"
        );
        let links = vec![Stored {
            path: PathBuf::from("/w/notes/a.org"),
            search: Some("*Intro".into()),
            description: "Intro".into(),
        }];
        assert_eq!(
            org_text(&links, Some(Path::new("/w/notes/doc.org"))),
            "[[file:a.org::*Intro][Intro]]"
        );
    }
}
