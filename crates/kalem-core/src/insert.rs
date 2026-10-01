//! Doom's `SPC i` (T2.7i.13): a character by its Unicode name, an emoji
//! by name, the file's name or path, a text from the clipboard history
//! or a Vim register, each through the palette's list, inserted at the
//! cursor by `insert.text`.

use std::sync::OnceLock;

use crate::palette::PaletteItem;

/// The command that inserts its `text` argument.
pub const TEXT: &str = "insert.text";

fn item(text: &str, title: String, category: &str, also: String) -> PaletteItem {
    PaletteItem {
        id: crate::palette::invocation(TEXT, &serde_json::json!({ "text": text })),
        title,
        category: category.to_string(),
        keys: String::new(),
        also,
    }
}

/// Every character with a Unicode name (not the ones named by number:
/// CJK ideographs, Hangul syllables), as a list to choose from: the
/// character and its name. Built once.
pub fn unicode_items() -> Vec<PaletteItem> {
    static ITEMS: OnceLock<Vec<PaletteItem>> = OnceLock::new();
    ITEMS
        .get_or_init(|| {
            let category = crate::tr!("category-unicode");
            (0u32..0x3_0000)
                .filter_map(char::from_u32)
                .filter(|c| !c.is_control())
                .filter_map(|c| {
                    let name = unicode_names2::name(c)?.to_string();
                    if name.starts_with("CJK ")
                        || name.starts_with("HANGUL SYLLABLE")
                        || name.starts_with("TANGUT")
                        || name.contains("COMPATIBILITY IDEOGRAPH")
                    {
                        return None;
                    }
                    let shown = if c.is_whitespace() {
                        format!("U+{:04X}", c as u32)
                    } else {
                        c.to_string()
                    };
                    Some(item(
                        &c.to_string(),
                        format!("{shown}  {}", name.to_lowercase()),
                        &category,
                        format!("U+{:04X}", c as u32),
                    ))
                })
                .collect()
        })
        .clone()
}

/// Every emoji with its name and shortcodes (`:smile:`).
pub fn emoji_items() -> Vec<PaletteItem> {
    let category = crate::tr!("category-emoji");
    emojis::iter()
        .map(|e| {
            let codes: Vec<String> = e.shortcodes().map(|s| format!(":{s}:")).collect();
            item(
                e.as_str(),
                format!("{}  {}", e.as_str(), e.name()),
                &category,
                codes.join(" "),
            )
        })
        .collect()
}

/// Texts to choose from, each shortened to its first line for the list.
pub fn text_items(texts: &[(String, String)], category: &str) -> Vec<PaletteItem> {
    texts
        .iter()
        .map(|(label, text)| {
            let first = text.lines().next().unwrap_or("").trim();
            let lines = text.lines().count();
            let title = if lines > 1 {
                format!("{label}{first} … ({lines} lines)")
            } else {
                format!("{label}{first}")
            };
            item(text, title, category, String::new())
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_to_choose_from() {
        let items = unicode_items();
        assert!(items.len() > 20_000, "{}", items.len());
        let arrow = items
            .iter()
            .find(|i| i.title.ends_with("rightwards arrow"))
            .unwrap();
        assert!(arrow.id.contains('→'));
        let smile = emoji_items();
        assert!(smile.iter().any(|i| i.also.contains(":smile:")));
        let t = text_items(&[("a  ".into(), "one\ntwo".into())], "x");
        assert_eq!(t[0].title, "a  one … (2 lines)");
    }
}
