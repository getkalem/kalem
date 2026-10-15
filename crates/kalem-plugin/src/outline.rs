//! A document's outline from a plugin (API 0.2.18): its headings, for the
//! outline sidebar of both editors. A plugin declares its outlines in its
//! manifest (`"outlines": [{"id", "types"}]`), builds with the feature
//! `outline`, and answers [`crate::kalem::Plugin::outline`]: from the text
//! alone, quickly, as the editor waits for it.
//!
//! ```ignore
//! fn outline(_id: &str, r: &outline::Request) -> Vec<outline::Heading> {
//!     let mut at = 0;
//!     let mut out = Vec::new();
//!     for line in r.text.split_inclusive('\n') {
//!         if let Some(title) = line.strip_prefix("* ") {
//!             out.push(outline::heading(1, title.trim(), at));
//!         }
//!         at += line.len();
//!     }
//!     out
//! }
//! ```

#[cfg(feature = "outline")]
pub use crate::extension::exports::kalem::plugin::outline::{Heading, Request};

/// A heading of level `level` (1 the outermost) titled `title`, starting
/// at byte `start` of the text.
#[cfg(feature = "outline")]
pub fn heading(level: u32, title: &str, start: usize) -> Heading {
    Heading {
        level,
        title: title.to_string(),
        start: start as u64,
    }
}
