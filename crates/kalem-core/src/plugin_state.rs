//! What a plugin keeps between sessions (plugin API 0.2.17, `state`):
//! small values by key, the plugin's alone, in Kalem's state folder
//! (`plugin-state/ID.json`): the pages opened last, a view's toggles, a
//! review under way. Not settings, which the user sets, nor the user's
//! files. A value is written at once; a plugin's are read once a session.

use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;
use std::sync::{Mutex, RwLock};

/// The longest key, in bytes.
pub const MAX_KEY: usize = 256;

/// What one plugin keeps in all, keys and values, in bytes.
pub const MAX_TOTAL: usize = 1 << 20;

/// The folder the plugins' state is kept in, when not the state folder's
/// (tests).
static DIR: RwLock<Option<PathBuf>> = RwLock::new(None);

/// The plugins' state read this session, by plugin.
static KEPT: Mutex<Option<HashMap<String, BTreeMap<String, String>>>> = Mutex::new(None);

/// Keeps the plugins' state in `dir` rather than in the state folder (a
/// test's), forgetting what was read.
pub fn set_dir(dir: Option<PathBuf>) {
    *DIR.write().unwrap_or_else(|e| e.into_inner()) = dir;
    *KEPT.lock().unwrap_or_else(|e| e.into_inner()) = None;
}

/// The file of plugin `plugin`'s state: its ID, letters, digits, `.`,
/// `-` and `_` kept, anything else `_`.
fn file(plugin: &str) -> Option<PathBuf> {
    let dir = DIR
        .read()
        .unwrap_or_else(|e| e.into_inner())
        .clone()
        .or_else(|| crate::logging::state_dir().map(|d| d.join("plugin-state")))?;
    let name: String = plugin
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_') {
                c
            } else {
                '_'
            }
        })
        .collect();
    (!name.is_empty()).then(|| dir.join(format!("{name}.json")))
}

/// Runs `f` on plugin `plugin`'s state, read from its file the first time.
fn with<R>(plugin: &str, f: impl FnOnce(&mut BTreeMap<String, String>) -> R) -> R {
    let mut kept = KEPT.lock().unwrap_or_else(|e| e.into_inner());
    let all = kept.get_or_insert_with(HashMap::new);
    let state = all.entry(plugin.to_string()).or_insert_with(|| {
        file(plugin)
            .and_then(|f| std::fs::read_to_string(f).ok())
            .and_then(|t| serde_json::from_str::<BTreeMap<String, String>>(&t).ok())
            .unwrap_or_default()
    });
    f(state)
}

/// The value plugin `plugin` keeps under `key`, if any.
pub fn get(plugin: &str, key: &str) -> Option<String> {
    with(plugin, |s| s.get(key).cloned())
}

/// The keys plugin `plugin` keeps values under, in order.
pub fn keys(plugin: &str) -> Vec<String> {
    with(plugin, |s| s.keys().cloned().collect())
}

/// Keeps `value` under `key` for plugin `plugin`, or forgets it (none),
/// written to its file at once. Refused: an empty key or one past
/// [`MAX_KEY`], or more than [`MAX_TOTAL`] kept in all.
pub fn set(plugin: &str, key: &str, value: Option<&str>) -> Result<(), String> {
    if key.is_empty() || key.len() > MAX_KEY {
        return Err(format!("A key of a plugin's state is 1 to {MAX_KEY} bytes"));
    }
    let path = file(plugin).ok_or("Kalem has no state folder")?;
    with(plugin, |s| {
        let mut next = s.clone();
        match value {
            Some(v) => {
                next.insert(key.to_string(), v.to_string());
            }
            None => {
                next.remove(key);
            }
        }
        if next == *s {
            return Ok(());
        }
        let total: usize = next.iter().map(|(k, v)| k.len() + v.len()).sum();
        if total > MAX_TOTAL {
            return Err(format!(
                "A plugin keeps at most {} KB of state",
                MAX_TOTAL / 1024
            ));
        }
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        let text = serde_json::to_string_pretty(&next).map_err(|e| e.to_string())?;
        crate::files::write(&path, text.as_bytes(), crate::files::SaveOptions::default())
            .map_err(|e| e.to_string())?;
        *s = next;
        Ok(())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kept_by_plugin_across_sessions_within_limits() {
        let dir = std::env::temp_dir().join(format!("kalem-plugin-state-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        set_dir(Some(dir.clone()));
        assert_eq!(get("graph", "recent"), None);
        set("graph", "recent", Some("[\"/g/a.md\"]")).unwrap();
        set("graph", "view:/g", Some("{\"journals\":true}")).unwrap();
        set("git", "recent", Some("other")).unwrap();
        // Each plugin its own, in a file of its own.
        assert_eq!(get("graph", "recent").as_deref(), Some("[\"/g/a.md\"]"));
        assert_eq!(get("git", "recent").as_deref(), Some("other"));
        assert_eq!(keys("graph"), ["recent", "view:/g"]);
        assert!(dir.join("graph.json").exists() && dir.join("git.json").exists());
        // Read again in the next session; a value forgotten.
        set_dir(Some(dir.clone()));
        assert_eq!(
            get("graph", "view:/g").as_deref(),
            Some("{\"journals\":true}")
        );
        set("graph", "view:/g", None).unwrap();
        set_dir(Some(dir.clone()));
        assert_eq!(keys("graph"), ["recent"]);
        // The limits.
        assert!(set("graph", "", Some("x")).is_err());
        assert!(set("graph", &"k".repeat(MAX_KEY + 1), Some("x")).is_err());
        assert!(set("graph", "big", Some(&"x".repeat(MAX_TOTAL))).is_err());
        assert_eq!(get("graph", "big"), None);
        // A plugin's ID made a file's name.
        set("../evil", "k", Some("v")).unwrap();
        assert!(dir.join(".._evil.json").exists());
        set_dir(None);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
