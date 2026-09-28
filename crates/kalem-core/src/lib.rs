//! Editor state shared by Kalem's frontends: documents, text, modes (and,
//! in later steps, commands, keymaps, settings and events).
//!
//! Frontends read documents and change them only through this crate.

mod builtin;
pub mod command;
pub mod dates;
pub mod dired;
pub mod document;
pub mod events;
pub mod files;
pub mod find;
pub mod formulas;
pub mod input;
pub mod keymap;
pub mod keys;
pub mod l10n;
pub mod logging;
pub mod math;
pub mod mode;
pub mod palette;
pub mod paste;
pub mod projects;
pub mod rich;
pub mod settings;
pub mod stats;
pub mod text;
pub mod theme;
pub mod view;
pub mod vim;
pub mod when;

pub use builtin::export_dialog_items;
pub use command::{
    Command, CommandError, CommandHandler, CommandRegistry, CommandResult, EditorContext, Request,
};
pub use document::{DocumentState, LineEnding, Metadata};
pub use events::{DocumentId, Event, EventBus, EventKind};
pub use keymap::{Keymap, Lookup, Profile};
pub use mode::DocumentMode;
pub use settings::Config;
pub use text::Text;
