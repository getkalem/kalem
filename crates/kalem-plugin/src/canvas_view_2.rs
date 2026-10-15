//! Canvases whose shapes' text has a size of its own (API 0.2.19): as
//! [`crate::canvas_view`], each shape with the size its text is written
//! in, in the canvas's units (a whiteboard's pixels). The window draws it
//! at that size times the zoom; the terminal a character a cell.
//!
//! ```ignore
//! use kalem_plugin::canvas_view::ShapeKind;
//! use kalem_plugin::canvas_view_2::{self, Canvas};
//! let mut title = canvas_view_2::shape("t", ShapeKind::Text, [0.0, 0.0, 300.0, 60.0], "Plans");
//! title.text_size = Some(48.0);
//! let c = Canvas { shapes: vec![title], connectors: Vec::new(), command: "notes.openShape".into() };
//! canvas_view_2::open(&kalem_plugin::documents::Spec::new("notes.board", "", "Board", "notes-board"), &c, "Plans\n")?;
//! ```

use crate::canvas_view::ShapeKind;
use crate::extension::kalem::plugin::canvas_view_2 as api;
pub use crate::extension::kalem::plugin::canvas_view_2::{Canvas, Shape};

/// A shape of `kind` at `rect` (left, top, width, height) with `text`, in
/// the theme's colors and the window's text size, leading nowhere.
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
        text_size: None,
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
