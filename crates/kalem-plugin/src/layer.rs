//! Layers over a mode's view (API 0.2.10): what a plugin changes in the
//! way a core mode draws a document. A plugin declares its layers in its
//! manifest (`"layers": [{"id", "markers", "modes"}]`), builds with the
//! feature `layer`, and answers [`crate::kalem::Plugin::overlays`]; it
//! calls [`refresh`] when what its layers show changed. With the feature
//! `layer-2` (API 0.2.13) it answers
//! [`crate::kalem::Plugin::overlays_2`] instead, which adds lines drawn
//! under a line of the text.

#[cfg(feature = "layer")]
pub use crate::extension::exports::kalem::plugin::layer::{
    LineEffect, Lines, OverlaySet, Replacement, Span, SpanEffect, SpanStyle,
};

/// What a layer changes with the feature `layer-2` (API 0.2.13): the
/// spans and lines of [`OverlaySet`], and lines under lines
/// ([`Below`]), which [`crate::kalem::Plugin::overlays_2`] gives.
#[cfg(feature = "layer-2")]
pub use crate::extension::exports::kalem::plugin::layer_2::{
    Below, OverlaySet as OverlaySet2, Piece, VirtualLine,
};

/// What the plugin's layers show changed (a file a document refers to):
/// every document they serve asks again.
pub fn refresh() {
    crate::extension::kalem::plugin::layers::refresh();
}
