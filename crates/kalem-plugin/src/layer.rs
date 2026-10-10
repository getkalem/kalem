//! Layers over a mode's view (API 0.2.10): what a plugin changes in the
//! way a core mode draws a document. A plugin declares its layers in its
//! manifest (`"layers": [{"id", "markers", "modes"}]`), builds with the
//! feature `layer`, and answers [`crate::kalem::Plugin::overlays`]; it
//! calls [`refresh`] when what its layers show changed.

#[cfg(feature = "layer")]
pub use crate::extension::exports::kalem::plugin::layer::{
    LineEffect, Lines, OverlaySet, Replacement, Span, SpanEffect, SpanStyle,
};

/// What the plugin's layers show changed (a file a document refers to):
/// every document they serve asks again.
pub fn refresh() {
    crate::extension::kalem::plugin::layers::refresh();
}
