//! The document a plugin's command runs in (design §11.4's `editor`):
//! read as the command found it, edited when the command returns, the
//! edits one undo step. A headline or a table is named by its start.
//!
//! ```ignore
//! use kalem_plugin::editor;
//!
//! for h in editor::find(&editor::Query { tag: Some("work".into()), todo: None, property: None }) {
//!     editor::set_todo(h.start, Some("DONE"))?;
//! }
//! editor::transact("Close the work items");
//! ```

pub use crate::extension::kalem::plugin::editor::*;
pub use crate::extension::kalem::plugin::kalem::Range;
