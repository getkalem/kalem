//! Plugins' formatters (plugin API 0.2.16, the `formatter` export): each
//! declared in its plugin's manifest (`"formatters": [{"id": "journal",
//! "types": ["hledger", "ledger"]}]`) and asked through the plugins'
//! bridge, on the editor's thread, by Format Document and
//! `editor.format_on_save`, while its plugin runs. A document's type is
//! one of [`super::names`]: its language, extension or mode.

use std::path::Path;
use std::sync::{Arc, Mutex, MutexGuard};

use crate::DocumentState;

/// A plugin's formatter, as its manifest declares it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginFormatterSpec {
    /// The plugin, by its short ID.
    pub plugin: String,
    /// The formatter's name, which the plugin is asked by.
    pub id: String,
    /// The types it formats (lower case, as `formatters.TYPE` names them);
    /// all when empty.
    pub types: Vec<String>,
}

/// What a plugin's formatter gave.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Answer {
    /// The whole text, formatted.
    Text(String),
    /// Why not, and where (a byte of the text) when it says.
    Refused { message: String, at: Option<usize> },
    /// The plugin was busy with another call: not asked.
    Busy,
    /// The plugin is gone, or failed.
    Failed(String),
}

/// Asks plugin `spec.plugin` to format `text` of the file at `path`, of
/// type `kind`: the plugins' bridge (`crate::extensions`).
pub type Provider = dyn Fn(&PluginFormatterSpec, Option<&Path>, &str, &str) -> Answer + Send + Sync;

struct Registry {
    formatters: Vec<PluginFormatterSpec>,
    provider: Option<Arc<Provider>>,
}

static REGISTRY: Mutex<Registry> = Mutex::new(Registry {
    formatters: Vec::new(),
    provider: None,
});

fn registry() -> MutexGuard<'static, Registry> {
    REGISTRY.lock().unwrap_or_else(|e| e.into_inner())
}

/// Adds a plugin's formatter, in place of one of the same plugin and ID.
pub fn register(spec: PluginFormatterSpec) {
    let mut r = registry();
    r.formatters
        .retain(|f| !(f.plugin == spec.plugin && f.id == spec.id));
    r.formatters.push(spec);
}

/// Takes plugin `plugin`'s formatters away (it stopped).
pub fn remove_plugin(plugin: &str) {
    registry().formatters.retain(|f| f.plugin != plugin);
}

/// The bridge the formatters are asked through.
pub fn set_provider(provider: Arc<Provider>) {
    registry().provider = Some(provider);
}

/// The plugin formatter of a document of types `names` ([`super::names`]):
/// the first registered that formats one of them.
pub fn for_names(names: &[String]) -> Option<PluginFormatterSpec> {
    registry()
        .formatters
        .iter()
        .find(|f| f.types.is_empty() || f.types.iter().any(|t| names.contains(t)))
        .cloned()
}

/// The plugin formatter of `doc`'s type, if a running plugin has one.
pub fn for_doc(doc: &DocumentState) -> Option<PluginFormatterSpec> {
    for_names(&super::names(doc))
}

/// `doc`'s text as `spec`'s plugin formats it.
pub fn format(spec: &PluginFormatterSpec, doc: &DocumentState) -> Answer {
    let Some(provider) = registry().provider.clone() else {
        return Answer::Failed(crate::tr!("fmt-plugin-gone", plugin = spec.plugin.as_str()));
    };
    let names = super::names(doc);
    let kind = names
        .iter()
        .find(|n| spec.types.contains(n))
        .or(names.first())
        .cloned()
        .unwrap_or_default();
    provider(spec, doc.meta.path.as_deref(), doc.text().as_str(), &kind)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn found_by_type_and_asked() {
        let spec = |types: &[&str]| PluginFormatterSpec {
            plugin: "fmtest".into(),
            id: "lines".into(),
            types: types.iter().map(|t| (*t).to_string()).collect(),
        };
        register(spec(&["fmtestlang"]));
        // The same plugin and ID once.
        register(spec(&["fmtestlang", "fmt2"]));
        assert_eq!(
            for_names(&["fmtestlang".into()]).map(|s| s.types.len()),
            Some(2)
        );
        assert!(for_names(&["markdown".into()]).is_none());
        remove_plugin("fmtest");
        assert!(for_names(&["fmtestlang".into()]).is_none());
    }
}
