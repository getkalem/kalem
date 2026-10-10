//! Completion by a plugin (API 0.2.11): items it offers at the cursor in
//! the menu Kalem shows with its own. A plugin declares its completers in
//! its manifest (`"completers": [{"id", "types", "layers", "triggers",
//! "word", "replaces", "priority"}]`), builds with the feature
//! `completer`, and answers [`crate::kalem::Plugin::complete`]: quickly,
//! as it is asked while the user types.

#[cfg(feature = "completer")]
pub use crate::extension::exports::kalem::plugin::completer::{Item, ItemKind, Request};

/// An item replacing the text from byte `start` of the document to the
/// cursor with `insert`, the cursor after it.
#[cfg(feature = "completer")]
pub fn item(label: &str, insert: &str, start: u64, kind: ItemKind) -> Item {
    Item {
        label: label.to_string(),
        insert: insert.to_string(),
        start,
        cursor: None,
        kind,
        detail: String::new(),
        documentation: None,
    }
}
