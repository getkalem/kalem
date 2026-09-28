//! The properties of an entry as a list to edit (Edit Properties): each
//! `KEY: value` of its property drawer, chosen to change its value, and a
//! new one.

use org_model::Document;
use org_syntax::ast::{self, AstNode};

use crate::palette::{PaletteItem, invocation};

/// The heading the entry at `pos` starts with.
fn heading(doc: &Document, pos: usize) -> Option<ast::Headline> {
    let root = doc.parse().syntax();
    let offset =
        org_syntax::TextSize::try_from(pos.min(usize::from(root.text_range().end()))).ok()?;
    let token = root
        .token_at_offset(offset)
        .left_biased()
        .or_else(|| root.token_at_offset(offset).right_biased())?;
    token.parent_ancestors().find_map(ast::Headline::cast)
}

/// The properties in the drawer of the entry at `pos`, in order.
pub fn entry_properties(doc: &Document, pos: usize) -> Vec<(String, String)> {
    heading(doc, pos).map_or_else(Vec::new, |h| h.properties())
}

/// The value of property `key` in the drawer of the entry at `pos`.
pub fn value(doc: &Document, pos: usize, key: &str) -> Option<String> {
    entry_properties(doc, pos)
        .into_iter()
        .find(|(k, _)| k.eq_ignore_ascii_case(key))
        .map(|(_, v)| v)
}

/// The list: each property, whose value choosing it asks for, then a new
/// property and, for each, its removal.
pub fn items(doc: &Document, pos: usize) -> Vec<PaletteItem> {
    let props = entry_properties(doc, pos);
    let item = |id: String, title: String, category: String| PaletteItem {
        id,
        title,
        category,
        keys: String::new(),
        also: String::new(),
    };
    let mut out: Vec<PaletteItem> = props
        .iter()
        .map(|(k, v)| {
            item(
                invocation("org.property.set", &serde_json::json!({ "key": k })),
                format!("{k}: {v}"),
                crate::l10n::tr("properties-change"),
            )
        })
        .collect();
    out.push(item(
        "org.property.set".into(),
        crate::l10n::tr("properties-new"),
        crate::l10n::tr("properties-add"),
    ));
    out.extend(props.iter().map(|(k, _)| {
        item(
            invocation("org.property.delete", &serde_json::json!({ "key": k })),
            crate::tr!("properties-delete", key = k.as_str()),
            crate::l10n::tr("properties-remove"),
        )
    }));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn listing() {
        let d = Document::new(org_syntax::parse(
            "* A\n:PROPERTIES:\n:ID: 42\n:Effort: 1:00\n:END:\nBody\n* B\n",
        ));
        assert_eq!(
            entry_properties(&d, 40),
            [
                ("ID".to_string(), "42".to_string()),
                ("Effort".into(), "1:00".into())
            ]
        );
        assert_eq!(value(&d, 2, "effort").as_deref(), Some("1:00"));
        assert!(entry_properties(&d, 52).is_empty());
        let items = items(&d, 2);
        assert_eq!(items.len(), 5);
        assert_eq!(items[0].title, "ID: 42");
        assert_eq!(
            crate::palette::split_invocation(&items[1].id),
            ("org.property.set", serde_json::json!({"key": "Effort"}))
        );
        assert_eq!(items[2].id, "org.property.set");
        assert_eq!(
            crate::palette::split_invocation(&items[4].id),
            ("org.property.delete", serde_json::json!({"key": "Effort"}))
        );
    }
}
