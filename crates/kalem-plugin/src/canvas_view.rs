//! Documents drawn as a canvas of shapes where the plugin places them
//! (API 0.2.16): a whiteboard, a canvas, a diagram. The plugin gives the
//! shapes (their place, size, kind, text, colors and where opening one
//! leads) and the connectors between them, and a text; the editor draws
//! them as placed, and opening a shape runs the canvas's command with
//! `{"shape": target}`.
//!
//! ```ignore
//! use kalem_plugin::canvas_view::{self, Canvas, ShapeKind};
//! let c = Canvas {
//!     shapes: vec![canvas_view::shape("a", ShapeKind::Box(4.0), [0.0, 0.0, 120.0, 60.0], "A page")],
//!     connectors: Vec::new(),
//!     command: "notes.openShape".into(),
//! };
//! let spec = kalem_plugin::documents::Spec::new("notes.board", "", "Board", "notes-board");
//! canvas_view::open(&spec, &c, "A page\n")?;
//! ```

use crate::extension::kalem::plugin::canvas_view as api;
pub use crate::extension::kalem::plugin::canvas_view::{
    Canvas, Connector, Point, Shape, ShapeKind,
};

/// A shape of `kind` at `rect` (left, top, width, height) with `text`, in
/// the theme's colors, leading nowhere.
pub fn shape(id: &str, kind: ShapeKind, rect: [f32; 4], text: &str) -> Shape {
    Shape {
        id: id.to_string(),
        kind,
        x: rect[0],
        y: rect[1],
        width: rect[2],
        height: rect[3],
        text: text.to_string(),
        fill: String::new(),
        stroke: String::new(),
        dashed: false,
        target: None,
    }
}

/// Shows `c` as the plugin's document `spec`, `text` its source; its
/// number.
pub fn open(spec: &crate::documents::Spec, c: &Canvas, text: &str) -> Result<u64, String> {
    api::open(spec.inner(), c, text)
}

/// Writes document `doc` anew, the view staying where it looks.
pub fn set(doc: u64, c: &Canvas, text: &str) -> Result<(), String> {
    api::set(doc, c, text)
}
