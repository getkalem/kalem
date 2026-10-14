//! `kalem.problems` (API 0.2.14): what a plugin finds wrong in files, its
//! tool's errors and warnings, drawn as a language server's diagnostics
//! are: the wavy line, the line number's color, the status line, the
//! hover and the problems list. A plugin sets a file's problems whole, as
//! the file is on disk, and sets them again when they change.
//!
//! ```ignore
//! use kalem_plugin::problems::{self, Problem, Severity};
//!
//! problems::set(
//!     "/home/a/books/2026.journal",
//!     &[Problem {
//!         line: 12,
//!         column: 0,
//!         length: 0,
//!         severity: Severity::Error,
//!         message: "This transaction is unbalanced".into(),
//!         source: Some("hledger".into()),
//!     }],
//! )?;
//! ```

use crate::extension::kalem::plugin::problems as api;

pub use crate::extension::kalem::plugin::problems::{Problem, Severity};

/// The problems of the file at `path` (absolute), in place of the
/// plugin's earlier ones there; none takes them away.
pub fn set(path: &str, problems: &[Problem]) -> Result<(), String> {
    api::set(path, problems)
}

/// Takes the plugin's problems away from the file at `path`, or from
/// every file.
pub fn clear(path: Option<&str>) {
    api::clear(path);
}
