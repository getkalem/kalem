//! Copy as rich text and Copy as HTML: the selection exported with the
//! HTML back-end, for pasting into mail, word processors and web pages.

use std::path::Path;

/// The HTML of the Org text `fragment` (a selection, or a whole
/// document): its body as the HTML export writes it, without a table of
/// contents or section numbers; `file` resolves relative links.
pub fn html(fragment: &str, file: Option<&Path>) -> Result<String, String> {
    let settings = org_export::Settings {
        body_only: true,
        input_file: file.map(Path::to_path_buf),
        now: None,
        subtree: None,
        math: Some(crate::math::export_renderer()),
        options: Some("toc:nil num:nil tex:svg".to_string()),
    };
    let out = org_export::export(fragment, &org_export::html::Html, &settings)?;
    Ok(out.trim().to_string())
}

/// What the copy commands copy: the selection of `doc`, or the whole
/// document when nothing is selected.
pub fn selection_text(doc: &crate::DocumentState) -> String {
    let (a, b) = (doc.selection.anchor, doc.selection.head);
    let (a, b) = (a.min(b), a.max(b));
    let text = doc.text().as_str();
    if a == b {
        text.to_string()
    } else {
        text[a..b].to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selections_as_html() {
        let h = html(
            "Some *bold* and /italic/ [[https://x.org][a link]].\n\n- one\n- two\n",
            None,
        )
        .unwrap();
        assert!(
            h.contains("<b>bold</b>") && h.contains("<i>italic</i>"),
            "{h}"
        );
        assert!(h.contains("<a href=\"https://x.org\">a link</a>"), "{h}");
        assert!(h.contains("<li>one</li>"), "{h}");
        assert!(
            !h.contains("<html") && !h.contains("table-of-contents"),
            "{h}"
        );
        let h = html("* Heading\ntext\n", None).unwrap();
        assert!(
            h.contains("<h2") && h.contains("Heading") && !h.contains("1 "),
            "{h}"
        );
    }
}
