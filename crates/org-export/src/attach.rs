//! `org-attach-expand-links`, which Org runs before parsing a document for
//! export: every `attachment:` link becomes a `file:` link to the file in
//! its heading's attachment folder, so that every back-end exports it as
//! the file it names.

use org_syntax::SyntaxKind::*;
use org_syntax::ast::{self, AstNode};

/// Where `org-attach` keeps the attachments of the heading holding `n`:
/// its `DIR` (or `ATTACH_DIR`) property, else `data/` and its `ID` split
/// after two characters (`org-attach-id-uuid-folder-format`).
fn attachment_dir(n: &org_syntax::SyntaxNode) -> Option<String> {
    for h in n.ancestors().filter_map(ast::Headline::cast) {
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
        if let Some(id) = get("ID") {
            let split = id.char_indices().nth(2).map_or(id.len(), |(i, _)| i);
            return Some(format!("data/{}/{}", &id[..split], &id[split..]));
        }
    }
    None
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
        let dir = attachment_dir(&n)
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
}
