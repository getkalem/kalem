//! What plugins keep on an open document ([`DocumentState::plugins`]):
//! the document a plugin writes and the styles of its text, the graph or
//! canvas it is drawn as, the marks beside its lines, the layer over its
//! mode's view and the line under a line being typed into. A document
//! keeps them whichever its mode is, so they are not its mode's state
//! (`crate::mode_state`) but a part of their own.
//!
//! [`DocumentState::plugins`]: crate::DocumentState::plugins

use std::path::PathBuf;

use crate::document::{GeneratedDoc, GutterMark, StyleSpan};

/// What plugins keep on a document.
#[derive(Debug, Default)]
pub struct PluginParts {
    /// The line under a line a layer draws being typed into, if any (API
    /// 0.2.15, [`crate::row_edit`]).
    pub row_edit: Option<crate::row_edit::RowEdit>,
    /// The line under a line to type into once its layer shows it: the
    /// block Enter made or Tab moved (API 0.2.18).
    pub row_resume: Option<crate::row_edit::Resume>,
    /// A document a plugin writes ([`GeneratedDoc`]).
    pub generated: Option<GeneratedDoc>,
    /// The graph a plugin's document is drawn as (`crate::graph_view`,
    /// plugin API 0.2.12): laid out and seen through the window, its
    /// text the source.
    pub graph_view: Option<Box<crate::graph_view::GraphView>>,
    /// The canvas a plugin's document is drawn as (`crate::canvas_view`,
    /// plugin API 0.2.16): its shapes where the plugin placed them, seen
    /// through the window, its text the source.
    pub canvas_view: Option<Box<crate::canvas_view::CanvasView>>,
    /// The styles its plugin gave the text of such a document, in order.
    pub(crate) generated_styles: Vec<StyleSpan>,
    /// The plugins' writes of marks read last, and the version of this
    /// file's marks taken.
    pub(crate) gutter_seen: (u64, u64),
    /// The marks plugins set beside its lines ([`GutterMark`]), by the
    /// start of their line when set, in order; they move with edits.
    pub(crate) gutter: Vec<(usize, GutterMark)>,
    /// The layer serving its folder, found for that path and the layers'
    /// generation.
    pub(crate) folder_layer: std::cell::RefCell<Option<(PathBuf, u64, Option<String>)>>,
    /// The overlays of the layer serving it (`crate::layers`): as last
    /// given, moved with the edits since, and the answer waited for.
    pub(crate) layer: std::cell::RefCell<crate::layers::DocLayer>,
}
