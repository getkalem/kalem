//! Plugins' outlines (plugin API 0.2.18, the `outline` export): a
//! document's headings from a plugin, for the outline sidebar of both
//! editors ([`crate::mode_view::outline_items`]) where the document's mode
//! and a language pack give none. Each is declared in its plugin's
//! manifest (`"outlines": [{"id": "journal", "types": ["hledger"]}]`) and
//! asked through the plugins' bridge once a version of the text, on a
//! thread of its own ([`set_background_provider`]): the answer is kept by
//! the document's file, and until it comes the sidebars show what they
//! showed, then draw again ([`crate::extensions::shown`]). A document's
//! type is one of [`crate::formatters::names`]: its language, extension
//! or mode.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};

use crate::DocumentState;
use crate::view::OutlineItem;

/// A plugin's outline, as its manifest declares it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginOutlineSpec {
    /// The plugin, by its short ID.
    pub plugin: String,
    /// The outline's name, which the plugin is asked by.
    pub id: String,
    /// The types it outlines (lower case); all when empty.
    pub types: Vec<String>,
}

/// What a plugin's outline gave.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Answer {
    /// The headings, in the text's order.
    Headings(Vec<OutlineItem>),
    /// The plugin was busy with another call: not asked.
    Busy,
    /// The plugin is gone, or failed.
    Failed(String),
}

/// Asks plugin `spec.plugin` for the outline of `text` of the file at
/// `path`, of type `kind`: the plugins' bridge (`crate::extensions`).
pub type Provider = dyn Fn(&PluginOutlineSpec, Option<&Path>, &str, &str) -> Answer + Send + Sync;

/// A file's last outline: its text's version and length, its headings
/// (none when the plugin failed).
type Kept = (u64, usize, Option<Vec<OutlineItem>>);

struct Registry {
    outlines: Vec<PluginOutlineSpec>,
    provider: Option<Arc<Provider>>,
    /// Whether the provider is asked on a thread of its own (the plugins'
    /// bridge, [`set_background_provider`]).
    background: bool,
    kept: BTreeMap<PathBuf, Kept>,
    /// The files whose outline is being asked on a thread, one at a time:
    /// of which version and length.
    asking: BTreeMap<PathBuf, (u64, usize)>,
}

static REGISTRY: Mutex<Registry> = Mutex::new(Registry {
    outlines: Vec::new(),
    provider: None,
    background: false,
    kept: BTreeMap::new(),
    asking: BTreeMap::new(),
});

fn registry() -> MutexGuard<'static, Registry> {
    REGISTRY.lock().unwrap_or_else(|e| e.into_inner())
}

/// Adds a plugin's outline, in place of one of the same plugin and ID.
pub fn register(spec: PluginOutlineSpec) {
    let mut r = registry();
    r.outlines
        .retain(|o| !(o.plugin == spec.plugin && o.id == spec.id));
    r.outlines.push(spec);
    r.kept.clear();
}

/// Takes plugin `plugin`'s outlines away (it stopped).
pub fn remove_plugin(plugin: &str) {
    let mut r = registry();
    r.outlines.retain(|o| o.plugin != plugin);
    r.kept.clear();
}

/// Sets what asks the outlines, asked on the editor's thread (a test's).
pub fn set_provider(provider: Arc<Provider>) {
    let mut r = registry();
    r.provider = Some(provider);
    r.background = false;
}

/// Sets the bridge to the plugins' outlines, asked on a thread of its
/// own.
pub fn set_background_provider(provider: Arc<Provider>) {
    let mut r = registry();
    r.provider = Some(provider);
    r.background = true;
}

/// The plugin outline of a document of types `names`
/// ([`crate::formatters::names`]): the first registered that outlines
/// one of them.
pub fn for_names(names: &[String]) -> Option<PluginOutlineSpec> {
    registry()
        .outlines
        .iter()
        .find(|o| o.types.is_empty() || o.types.iter().any(|t| names.contains(t)))
        .cloned()
}

/// The headings of `doc` from the plugin outline of its type, if a
/// running plugin has one: asked once a version of its text and kept by
/// its file; the last kept while the plugin is busy. A heading's start is
/// a byte of the text, on a character's boundary. Asked on a thread
/// ([`set_background_provider`]): none until the answer comes, the
/// editors keeping what they show (a document without a file has none).
pub fn outline_items(doc: &DocumentState) -> Option<Vec<OutlineItem>> {
    let names = crate::formatters::names(doc);
    let spec = for_names(&names)?;
    let text = doc.text().as_str();
    let now = (doc.version(), text.len());
    let path = doc.meta.path.clone();
    let mut r = registry();
    if let Some(p) = &path
        && let Some((v, len, items)) = r.kept.get(p)
        && (*v, *len) == now
    {
        return items.clone();
    }
    let provider = r.provider.clone()?;
    let kind = names
        .iter()
        .find(|n| spec.types.contains(n))
        .or(names.first())
        .cloned()
        .unwrap_or_default();
    if r.background {
        let path = path?;
        if !r.asking.contains_key(&path) {
            r.asking.insert(path.clone(), now);
            let text = doc.shared_text();
            crate::extensions::behind(move || {
                let answer = provider(&spec, Some(&path), &text, &kind);
                let mut r = registry();
                r.asking.remove(&path);
                match answer {
                    Answer::Headings(items) => {
                        r.kept
                            .insert(path, (now.0, now.1, Some(fitted(items, &text))));
                    }
                    Answer::Failed(_) => {
                        r.kept.insert(path, (now.0, now.1, None));
                    }
                    Answer::Busy => {}
                }
            });
        }
        return None;
    }
    drop(r);
    match provider(&spec, path.as_deref(), text, &kind) {
        Answer::Headings(items) => {
            let items = fitted(items, text);
            if let Some(p) = path {
                registry()
                    .kept
                    .insert(p, (now.0, now.1, Some(items.clone())));
            }
            Some(items)
        }
        Answer::Busy => path.and_then(|p| registry().kept.get(&p).and_then(|k| k.2.clone())),
        Answer::Failed(_) => None,
    }
}

/// `items` with their starts in `text`, on characters' boundaries.
fn fitted(mut items: Vec<OutlineItem>, text: &str) -> Vec<OutlineItem> {
    for i in &mut items {
        let mut at = i.start.min(text.len());
        while !text.is_char_boundary(at) {
            at -= 1;
        }
        i.start = at;
    }
    items
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn found_by_type() {
        let spec = |types: &[&str]| PluginOutlineSpec {
            plugin: "oltest".into(),
            id: "heads".into(),
            types: types.iter().map(|t| (*t).to_string()).collect(),
        };
        register(spec(&["oltestlang"]));
        // The same plugin and ID once.
        register(spec(&["oltestlang", "ol2"]));
        assert_eq!(
            for_names(&["oltestlang".into()]).map(|s| s.types.len()),
            Some(2)
        );
        assert!(for_names(&["markdown".into()]).is_none());
        remove_plugin("oltest");
        assert!(for_names(&["oltestlang".into()]).is_none());
    }
}
