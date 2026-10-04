//! `kalem.settings` of design §11.4: Kalem's settings read, the plugin's
//! own read and written (`[plugins."ID"]` of the user's `settings.toml`),
//! and changes watched. Values are JSON text.

use crate::extension::kalem::plugin::settings as api;
use crate::kalem::{Disposable, HANDLERS, Local};

/// Kalem's setting `key` (`editor.font_size`) as JSON.
pub fn get(key: &str) -> Option<String> {
    api::get(key)
}

/// The plugin's own setting `key` as JSON.
pub fn own(key: &str) -> Option<String> {
    api::own(key)
}

/// Sets the plugin's own setting `key` to `value` (JSON; `null` removes
/// it).
pub fn set(key: &str, value: &str) -> Result<(), String> {
    api::set(key, value)
}

/// Calls `changed` with the key each time setting `key` changes, Kalem's
/// or, with `own`, the plugin's.
pub fn watch(
    key: &str,
    own: bool,
    changed: impl FnMut(&str) + 'static,
) -> Result<Disposable, String> {
    let host = api::watch(key, own)?;
    let id = host.id();
    HANDLERS.with(|h| {
        h.borrow_mut()
            .watches
            .insert(id, (key.to_string(), own, Some(Box::new(changed))))
    });
    Ok(Disposable {
        host,
        local: Local::Watch(id),
    })
}
