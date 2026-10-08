//! `kalem.documents` (API 0.2.5): documents a plugin writes, read-only text
//! the editor shows as a document of its own, written again to change it.
//! A plugin's commands scoped to the document's kind act on what the
//! cursor is on (`editor::selected`); [`current`] says which document.
//!
//! ```ignore
//! use kalem_plugin::documents::{self, Spec};
//!
//! let doc = documents::open(
//!     &Spec::new("git.status", root, "Git: org", "git-status").language("diff"),
//!     &text,
//!     None,
//! )?;
//! ```

use crate::extension::kalem::plugin::documents as api;

pub use crate::extension::kalem::plugin::documents::DocumentSpec;

/// A document to show.
#[derive(Debug, Clone)]
pub struct Spec(DocumentSpec);

impl Spec {
    /// Document `id` (`git.status`) for `key` (what tells documents of one
    /// ID apart), titled `title`, of kind `kind` (`git-status`).
    pub fn new(id: &str, key: &str, title: &str, kind: &str) -> Spec {
        Spec(DocumentSpec {
            id: id.into(),
            key: key.into(),
            title: title.into(),
            kind: kind.into(),
            language: None,
        })
    }

    /// Its highlighter (`diff`).
    pub fn language(mut self, language: &str) -> Spec {
        self.0.language = Some(language.into());
        self
    }
}

/// Shows a document with `text`, the cursor at byte `cursor`; its number.
pub fn open(spec: &Spec, text: &str, cursor: Option<u64>) -> Result<u64, String> {
    api::open(&spec.0, text, cursor)
}

/// Writes document `doc` anew; the cursor at byte `cursor`, else on its
/// line. An error once the user closed it: the plugin forgets it.
pub fn set(doc: u64, text: &str, cursor: Option<u64>) -> Result<(), String> {
    api::set(doc, text, cursor)
}

/// The plugin's document the running command is in.
pub fn current() -> Option<u64> {
    api::current()
}

/// Closes document `doc`.
pub fn close(doc: u64) {
    api::close(doc);
}

pub use crate::extension::kalem::plugin::styled_documents::{Color, StyledSpan, TextStyle};

use crate::extension::kalem::plugin::styled_documents as styled;

impl TextStyle {
    /// Text in `color`, neither bold, italic nor underlined.
    pub fn color(color: Color) -> TextStyle {
        TextStyle {
            color,
            bold: false,
            italic: false,
            underline: false,
        }
    }

    /// The same, bold.
    pub fn bold(mut self) -> TextStyle {
        self.bold = true;
        self
    }
}

/// [`open`] with styles on stretches of the text (API 0.2.6): bytes
/// `start..end` and how they show; a span inside another wins over it.
pub fn open_styled(
    spec: &Spec,
    text: &str,
    cursor: Option<u64>,
    styles: &[StyledSpan],
) -> Result<u64, String> {
    styled::open(&spec.0, text, cursor, styles)
}

/// [`set`] with styles on stretches of the text (API 0.2.6).
pub fn set_styled(
    doc: u64,
    text: &str,
    cursor: Option<u64>,
    styles: &[StyledSpan],
) -> Result<(), String> {
    styled::set(doc, text, cursor, styles)
}
