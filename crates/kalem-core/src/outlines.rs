//! Plugins' outlines (plugin API 0.2.18, the `outline` export): a
//! document's headings from a plugin, for the outline sidebar of both
//! editors ([`crate::mode_view::outline_items`]) where the document's mode
//! and a language pack give none. Each is declared in its plugin's
//! manifest (`"outlines": [{"id": "journal", "types": ["hledger"]}]`) and
//! asked through the plugins' bridge, on the editor's thread, once a
//! version of the text: the answer is kept by the document's file, and
//! while the plugin is busy the last stays. A document's type is one of
//! [`crate::formatters::names`]: its language, extension or mode.

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

/// A file's last outline: its text's version and length, its headings.
type Kept = (u64, usize, Vec<OutlineItem>);

struct Registry {
    outlines: Vec<PluginOutlineSpec>,
    provider: Option<Arc<Provider>>,
    kept: BTreeMap<PathBuf, Kept>,
}

static REGISTRY: Mutex<Registry> = Mutex::new(Registry {
    outlines: Vec::new(),
    provider: None,
    kept: BTreeMap::new(),
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

/// The bridge the outlines are asked through.
pub fn set_provider(provider: Arc<Provider>) {
    registry().provider = Some(provider);
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
/// a byte of the text, on a character's boundary.
pub fn outline_items(doc: &DocumentState) -> Option<Vec<OutlineItem>> {
    let names = crate::formatters::names(doc);
    let spec = for_names(&names)?;
    let text = doc.text().as_str();
    let now = (doc.version(), text.len());
    let path = doc.meta.path.clone();
    if let Some(p) = &path
        && let Some((v, len, items)) = registry().kept.get(p)
        && (*v, *len) == now
    {
        return Some(items.clone());
    }
    let provider = registry().provider.clone()?;
    let kind = names
        .iter()
        .find(|n| spec.types.contains(n))
        .or(names.first())
        .cloned()
        .unwrap_or_default();
    match provider(&spec, path.as_deref(), text, &kind) {
        Answer::Headings(mut items) => {
            for i in &mut items {
                let mut at = i.start.min(text.len());
                while !text.is_char_boundary(at) {
                    at -= 1;
                }
                i.start = at;
            }
            if let Some(p) = path {
                registry().kept.insert(p, (now.0, now.1, items.clone()));
            }
            Some(items)
        }
        Answer::Busy => path.and_then(|p| registry().kept.get(&p).map(|k| k.2.clone())),
        Answer::Failed(_) => None,
    }
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
