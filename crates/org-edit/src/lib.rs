//! Structural editing of Org mode files.
//!
//! Every command reads the document ([`org_model::Document`]) and returns a
//! [`Transaction`]: replacements of text ranges, applied at once and undone
//! as one step. The text a command produces is the text the Emacs command
//! of the same name produces, which the Emacs differential tests check.

// A crash ends the user's work: no `unwrap`, `expect` or `panic!`
// outside tests but where an `#[expect]` says why it cannot happen
// (roadmap R2.2).
#![warn(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

pub mod archive;
mod buffer;
pub mod emphasis;
pub mod footnote;
pub mod format;
pub mod headline;
mod history;
pub mod insert;
pub mod list;
pub mod motion;
pub mod narrow;
pub mod property;
pub mod recalc;
pub mod sort;
pub mod style;
pub mod table;
pub mod tags;
pub mod timestamp;
pub mod todo;
pub mod toggle;
mod transaction;
pub mod typing;

pub use buffer::EditError;
pub use history::{ChangeKind, History, Replay};
pub use transaction::{Assoc, Edit, OverlapError, Selection, Transaction};
