//! `org-attach-expand-links`, which Org runs before parsing a document for
//! export: every `attachment:` link becomes a `file:` link to the file in
//! its heading's attachment folder, so that every back-end exports it as
//! the file it names.

use org_syntax::SyntaxKind::*;
use org_syntax::ast::{self, AstNode};

/// Where `org-attach` keeps the attachments of the heading holding `n`
/// (`org-attach-dir`): the heading's own `DIR` (or `ATTACH_DIR`)
/// property, else a folder in `data/` from its `ID`. Properties are not
/// inherited: `org-attach-use-inheritance` is `selective` and
/// `org-use-property-inheritance` nil. Of the folders the ID functions
/// give (`org-attach-id-to-path-function-list`: `ab/cdef`, `abcdef/gh`,
/// `__/a/abcdefgh`), the first that exists under `base`, else the first;
/// the first when `base` is not known. Relative to the document's folder.
pub fn attachment_dir(
    n: &org_syntax::SyntaxNode,
    base: Option<&std::path::Path>,
) -> Option<String> {
    let h = n.ancestors().find_map(ast::Headline::cast)?;
    let props = h.properties();
    let get = |k: &str| {
        props
            .iter()
            .find(|(key, _)| key.eq_ignore_ascii_case(k))
            .map(|(_, v)| v.trim().to_string())
            .filter(|v| !v.is_empty())
    };
    if let Some(d) = get("DIR").or_else(|| get("ATTACH_DIR")) {
        return Some(d);
    }
    let id = get("ID")?;
    // `ab/cdef` for an ID longer than `at` characters.
    let split = |at: usize| {
        let i = id.char_indices().nth(at).map(|(i, _)| i)?;
        Some(format!("{}/{}", &id[..i], &id[i..]))
    };
    let first = id.chars().next()?;
    let candidates: Vec<String> = [split(2), split(6), Some(format!("__/{first}/{id}"))]
        .into_iter()
        .flatten()
        .map(|c| format!("data/{c}"))
        .collect();
    let existing = base.and_then(|b| candidates.iter().find(|c| b.join(c).is_dir()));
    existing.or(candidates.first()).cloned()
}

/// `text` with its `attachment:` links expanded, as
/// `org-attach-expand-links` does: `[[attachment:F][D]]` becomes
/// `[[file:DIR/F][D]]`, the folder absolute from the document's (`file`);
/// a heading without an attachment folder, or a folder that does not
/// exist, leaves the name relative to the document's folder, as
/// `expand-file-name` does with no folder. `marks` (byte offsets) move
/// with the text.
pub(crate) fn expand(text: &str, file: Option<&std::path::Path>, marks: &mut [usize; 2]) -> String {
    if !text.contains("attachment:") {
        return text.to_string();
    }
    let parse = org_syntax::parse(text);
    let root = parse.syntax();
    let ctx = parse.context();
    let base = file
        .and_then(std::path::Path::parent)
        .map(std::path::Path::to_path_buf)
        .or_else(|| std::env::current_dir().ok())
        .unwrap_or_default();
    let mut edits: Vec<(std::ops::Range<usize>, String)> = Vec::new();
    for n in root.descendants().filter(|n| n.kind() == LINK) {
        let Some(link) = ast::Link::cast(n.clone()) else {
            continue;
        };
        let info = link.info(ctx);
        if !info.link_type.eq_ignore_ascii_case("attachment") {
            continue;
        }
        let path = info.path;
        let dir = attachment_dir(&n, Some(&base))
            .map(|d| base.join(d))
            .filter(|d| d.is_dir())
            .unwrap_or_else(|| base.clone());
        let full = dir.join(&path);
        let full = full.to_string_lossy().replace('\\', "/");
        let range = n.text_range();
        let (start, mut end) = (usize::from(range.start()), usize::from(range.end()));
        // The blanks after the link are kept.
        while end > start && text[..end].ends_with([' ', '\t']) {
            end -= 1;
        }
        let description = link
            .description()
            .map(|r| text[usize::from(r.start())..usize::from(r.end())].to_string());
        let new = match description {
            Some(d) => format!("[[file:{full}][{d}]]"),
            None => format!("[[file:{full}]]"),
        };
        edits.push((start..end, new));
    }
    let mut out = text.to_string();
    for (r, new) in edits.into_iter().rev() {
        let delta = new.len() as isize - r.len() as isize;
        for m in marks.iter_mut() {
            if *m >= r.end {
                *m = (*m as isize + delta) as usize;
            }
        }
        out.replace_range(r, &new);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn attachment_links_become_file_links() {
        let dir = std::env::temp_dir().join(format!("kalem-attach-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("data/ab/cdef")).unwrap();
        let file = dir.join("n.org");
        let text = "* H\n:PROPERTIES:\n:ID: abcdef\n:END:\n[[attachment:plan.png]] and [[attachment:doc.txt][the doc]].\n";
        let mut marks = [0, text.len()];
        let out = expand(text, Some(&file), &mut marks);
        let base = dir.to_string_lossy().replace('\\', "/");
        assert!(
            out.contains(&format!("[[file:{base}/data/ab/cdef/plan.png]] and")),
            "{out}"
        );
        assert!(
            out.contains(&format!("[[file:{base}/data/ab/cdef/doc.txt][the doc]]")),
            "{out}"
        );
        assert_eq!(marks[1], out.len());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn attachment_folders_as_org_attach_finds_them() {
        let dir = std::env::temp_dir().join(format!("kalem-attach-dirs-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("data/202409/30")).unwrap();
        let text = "* A\n:PROPERTIES:\n:ID: 20240930\n:END:\n** B\n[[attachment:x]]\n* C\n:PROPERTIES:\n:ID: 20240930\n:END:\n[[attachment:y]]\n* D\n:PROPERTIES:\n:ID: ab\n:DIR: here\n:END:\n[[attachment:z]]\n* E\n:PROPERTIES:\n:ID: ab\n:END:\n[[attachment:w]]\n";
        let p = org_syntax::parse(text);
        let links: Vec<_> = p
            .syntax()
            .descendants()
            .filter(|n| n.kind() == LINK)
            .collect();
        // Not inherited.
        assert_eq!(attachment_dir(&links[0], Some(&dir)), None);
        // The folder that exists, else the first.
        assert_eq!(
            attachment_dir(&links[1], Some(&dir)).as_deref(),
            Some("data/202409/30")
        );
        assert_eq!(
            attachment_dir(&links[1], None).as_deref(),
            Some("data/20/240930")
        );
        assert_eq!(attachment_dir(&links[2], None).as_deref(), Some("here"));
        // Too short to split: the fallback.
        assert_eq!(
            attachment_dir(&links[3], None).as_deref(),
            Some("data/__/a/ab")
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
