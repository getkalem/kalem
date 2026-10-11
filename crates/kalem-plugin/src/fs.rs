//! `kalem.fs` of design §11.4: files, reached only with a permission of
//! the manifest (`fs:read:workspace`, `fs:write:workspace`,
//! `fs:read:all`). A plugin that uses these functions imports the `fs`
//! interface, and Kalem refuses to load it without one.

use crate::extension::kalem::plugin::fs as api;

/// A text file's text.
pub fn read(path: &str) -> Result<String, String> {
    api::read(path)
}

/// Writes a text file.
pub fn write(path: &str, text: &str) -> Result<(), String> {
    api::write(path, text)
}

/// A folder's entries, folders ending in `/`.
pub fn list(dir: &str) -> Result<Vec<String>, String> {
    api::list(dir)
}

/// Moves the file or folder `from` to `to`, its folder made when
/// missing; refused when `to` exists (`fs-2`, API 0.2.12). Kalem's open
/// documents follow it.
pub fn rename(from: &str, to: &str) -> Result<(), String> {
    crate::extension::kalem::plugin::fs_2::rename(from, to)
}

/// Moves the file or folder `path` to the system's trash (`fs-2`, API
/// 0.2.12).
pub fn trash(path: &str) -> Result<(), String> {
    crate::extension::kalem::plugin::fs_2::trash(path)
}
