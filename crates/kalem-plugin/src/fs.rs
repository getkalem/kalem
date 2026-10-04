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
