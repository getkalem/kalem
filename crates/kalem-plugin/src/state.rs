//! What a plugin keeps between sessions (API 0.2.17): small values by
//! key, the plugin's alone, in Kalem's state folder (the pages opened
//! last, a view's toggles), kept on disk at once.
//!
//! ```ignore
//! kalem_plugin::state::set("recent", Some("[\"/notes/a.md\"]"))?;
//! let recent = kalem_plugin::state::get("recent");
//! ```

use crate::extension::kalem::plugin::state as api;

/// The value kept under `key`, if any.
pub fn get(key: &str) -> Option<String> {
    api::get(key)
}

/// Keeps `value` under `key`, or forgets it (none): an error for an empty
/// key or one past 256 bytes, or past 1 MB kept in all.
pub fn set(key: &str, value: Option<&str>) -> Result<(), String> {
    api::set(key, value)
}

/// The keys values are kept under, in order.
pub fn keys() -> Vec<String> {
    api::keys()
}
