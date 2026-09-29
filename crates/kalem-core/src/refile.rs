//! Refile: the headings a subtree can move under, for a picker.

use org_model::Document;

use crate::palette::PaletteItem;

/// The command that refiles the subtree at the cursor.
pub const REFILE: &str = "org.refile";

/// The headings the subtree at `point` can be refiled under: every
/// heading outside it, shown with its outline path (`Parent/Child`), as
/// `org-refile` lists them.
pub fn picker_items(doc: &Document, point: usize) -> Vec<PaletteItem> {
    let outline = doc.outline();
    let entries = &outline.entries;
    // The subtree at the cursor.
    let own = entries
        .iter()
        .rfind(|e| {
            !e.inlinetask
                && usize::from(e.range.start()) <= point
                && point < usize::from(e.range.end())
        })
        .map(|e| usize::from(e.range.start())..usize::from(e.range.end()));
    let mut out = Vec::new();
    for e in entries.iter().filter(|e| !e.inlinetask) {
        let start = usize::from(e.range.start());
        if own.as_ref().is_some_and(|r| r.contains(&start)) {
            continue;
        }
        let mut path = vec![e.raw_title.trim().to_string()];
        let mut parent = e.parent;
        while let Some(p) = parent {
            let pe = outline.get(p);
            path.push(pe.raw_title.trim().to_string());
            parent = pe.parent;
        }
        path.reverse();
        out.push(PaletteItem {
            id: crate::palette::invocation(REFILE, &serde_json::json!({ "target": start })),
            title: path.join("/"),
            category: String::new(),
            keys: String::new(),
            also: String::new(),
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn targets() {
        let t = "* A\n** B\n* C\n** D\n";
        let d = Document::new(org_syntax::parse(t));
        let titles: Vec<String> = picker_items(&d, 12).into_iter().map(|i| i.title).collect();
        assert_eq!(titles, ["A", "A/B"]);
        let items = picker_items(&d, 5);
        assert_eq!(
            items.iter().map(|i| i.title.as_str()).collect::<Vec<_>>(),
            ["A", "C", "C/D"]
        );
        assert!(items[1].id.contains("\"target\":9"), "{}", items[1].id);
    }
}
