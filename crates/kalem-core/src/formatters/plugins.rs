//! Plugins' formatters (plugin API 0.2.16, the `formatter` export): each
//! declared in its plugin's manifest (`"formatters": [{"id": "journal",
//! "types": ["hledger", "ledger"]}]`) and asked through the plugins'
//! bridge by Format Document and `editor.format_on_save`, while its
//! plugin runs: on a thread of its own, the editor waiting at most
//! [`WAIT`] for the text. A document's type is one of [`super::names`]:
//! its language, extension or mode.

use std::path::Path;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

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

/// How long Format Document waits for a plugin's formatter asked on a
/// thread: its call's time, and as long again for the plugin to end a
/// call of the drawing's it is in. A plugin busier than that is busy.
pub const WAIT: Duration =
    Duration::from_millis(2 * crate::extensions::DRAW_TIME.as_millis() as u64);

struct Registry {
    formatters: Vec<PluginFormatterSpec>,
    provider: Option<Arc<Provider>>,
    /// Whether the provider is asked on a thread of its own (the plugins'
    /// bridge, [`set_background_provider`]).
    background: bool,
}

static REGISTRY: Mutex<Registry> = Mutex::new(Registry {
    formatters: Vec::new(),
    provider: None,
    background: false,
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

/// Sets what asks the formatters, asked on the editor's thread (a
/// test's).
pub fn set_provider(provider: Arc<Provider>) {
    let mut r = registry();
    r.provider = Some(provider);
    r.background = false;
}

/// Sets the bridge to the plugins' formatters, asked on a thread of its
/// own and waited for at most [`WAIT`].
pub fn set_background_provider(provider: Arc<Provider>) {
    let mut r = registry();
    r.provider = Some(provider);
    r.background = true;
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

/// `doc`'s text as `spec`'s plugin formats it; busy when the plugins'
/// bridge does not answer within [`WAIT`].
pub fn format(spec: &PluginFormatterSpec, doc: &DocumentState) -> Answer {
    let (provider, background) = {
        let r = registry();
        (r.provider.clone(), r.background)
    };
    let Some(provider) = provider else {
        return Answer::Failed(crate::tr!("fmt-plugin-gone", plugin = spec.plugin.as_str()));
    };
    let names = super::names(doc);
    let kind = names
        .iter()
        .find(|n| spec.types.contains(n))
        .or(names.first())
        .cloned()
        .unwrap_or_default();
    if !background {
        return provider(spec, doc.meta.path.as_deref(), doc.text().as_str(), &kind);
    }
    let (send, answer) = std::sync::mpsc::channel();
    let spec_name = spec.plugin.clone();
    let (spec, path) = (spec.clone(), doc.meta.path.clone());
    let text = doc.shared_text();
    let asked = std::thread::Builder::new()
        .name("kalem-plugin-format".into())
        .spawn(move || {
            let _ = send.send(provider(&spec, path.as_deref(), &text, &kind));
        });
    if asked.is_err() {
        return Answer::Busy;
    }
    match answer.recv_timeout(WAIT) {
        Ok(a) => a,
        Err(std::sync::mpsc::RecvTimeoutError::Timeout) => Answer::Busy,
        Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
            Answer::Failed(crate::tr!("fmt-plugin-gone", plugin = spec_name.as_str()))
        }
    }
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
