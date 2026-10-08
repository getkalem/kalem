//! `kalem.decorations` (API 0.2.5): marks beside the lines of files, the
//! git plugin's added, changed and removed lines. A plugin sets a file's
//! marks whole, as the file is on disk; the editor moves them with the
//! edits made since, until the plugin sets them again.
//!
//! ```ignore
//! use kalem_plugin::decorations::{self, Mark};
//!
//! decorations::set_gutter("/home/a/org/src/lib.rs", &[(12, Mark::Changed), (40, Mark::Added)])?;
//! ```

use crate::extension::kalem::plugin::decorations as api;

pub use crate::extension::kalem::plugin::decorations::MarkKind as Mark;

/// Marks the lines (from 1; 0 for lines removed before the first) of the
/// file at `path`, in place of the plugin's earlier marks there; none
/// takes them away.
pub fn set_gutter(path: &str, marks: &[(u32, Mark)]) -> Result<(), String> {
    let marks: Vec<api::LineMark> = marks
        .iter()
        .map(|(line, kind)| api::LineMark {
            line: *line,
            kind: *kind,
        })
        .collect();
    api::set_gutter(path, &marks)
}

/// Takes the plugin's marks away from the file at `path`, or from every
/// file.
pub fn clear_gutter(path: Option<&str>) {
    api::clear_gutter(path);
}
