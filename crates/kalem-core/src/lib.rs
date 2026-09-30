//! Editor state shared by Kalem's frontends: documents, text, modes (and,
//! in later steps, commands, keymaps, settings and events).
//!
//! Frontends read documents and change them only through this crate.

pub mod affiliated;
pub mod bibtex;
mod builtin;
pub mod cite;
pub mod code;
pub mod command;
pub mod completers;
pub mod csv;
pub mod cursors;
pub mod dates;
pub mod dired;
pub mod document;
pub mod events;
pub mod files;
pub mod find;
pub mod formulas;
pub mod images;
pub mod input;
pub mod jobs;
pub mod keymap;
pub mod keys;
pub mod kinds;
pub mod l10n;
pub mod latex_build;
pub mod latex_check;
mod latex_complete;
pub mod latex_edit;
pub mod latex_fmt;
pub mod latex_table;
pub mod latex_templates;
pub mod latex_view;
pub mod lines;
pub mod links;
pub mod logging;
pub mod math;
pub mod mode;
pub mod palette;
pub mod pandoc;
pub mod paste;
pub mod pdf;
pub mod print;
pub mod projects;
pub mod properties;
pub mod refile;
pub mod rich;
pub mod rich_copy;
pub mod settings;
pub mod stats;
pub mod system;
pub mod text;
pub mod theme;
pub mod toc;
pub mod view;
pub mod vim;
pub mod when;

pub use builtin::export_dialog_items;
pub use command::{
    Command, CommandError, CommandHandler, CommandRegistry, CommandResult, EditorContext, Request,
};
pub use document::{DocumentState, LineEnding, Metadata};
/// The character encodings of files (`Metadata::encoding`).
pub use encoding_rs;
pub use events::{DocumentId, Event, EventBus, EventKind};
pub use keymap::{Keymap, Lookup, Profile};
pub use mode::DocumentMode;
pub use settings::Config;
pub use text::Text;
