//! File manager operations for text editors, like Emacs's Dired (Kalem
//! design §2.7): directory listings with sorting and hidden files, and
//! copy, move, trash and delete run in the background with progress,
//! cancellation and a choice for each conflict. Usable without the
//! editor.
//!
//! ```no_run
//! use kalem_fs::{ListOptions, read_dir};
//! let entries = read_dir(std::path::Path::new("."), &ListOptions::default())?;
//! for e in &entries {
//!     println!("{} {}", kalem_fs::permissions(e), e.name);
//! }
//! # Ok::<(), std::io::Error>(())
//! ```

mod job;
mod listing;
mod ops;

pub use job::{Job, Progress};
pub use listing::{
    Entry, Kind, ListOptions, SortKey, format_size, format_time, permissions, read_dir, sort,
};
pub use ops::{
    Conflict, OpKind, Operation, Outcome, chmod, conflicts, copy_path, delete_path, mkdir,
    move_path, symlink, touch, trash_paths, unique_name,
};
