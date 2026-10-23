//! Plugins' completers (plugin API 0.2.11, the `completer` export): each
//! declared in its plugin's manifest (`"completers": [{"id": "pages",
//! "triggers": ["[["], "layers": ["logseq"], "replaces": ["wiki"]}]`) and
//! asked through the plugins' bridge as the user types: on a thread of
//! its own, as a language server's completer is, within
//! [`PLUGIN_BUDGET`]. They join the core's completers in
//! [`super::Registry`] while their plugin runs.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use std::time::Duration;

use super::{Cancel, Completer, Context, Item, Trigger};
use crate::DocumentState;

/// A plugin's completer, as its manifest declares it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginCompleterSpec {
    /// The plugin, by its short ID.
    pub plugin: String,
    /// The completer's name, which the plugin is asked by.
    pub id: String,
    /// The text types it serves (§11.2); all when empty.
    pub types: Vec<String>,
    /// The plugin's layers (`crate::layers`) it serves: when there are
    /// any, it applies only in a document one of them serves.
    pub layers: Vec<String>,
    /// The strings before the cursor on its line that open its menu.
    pub triggers: Vec<String>,
    /// A word of this many letters being typed opens its menu too.
    pub word: Option<usize>,
    /// The completers it stands in for where it applies (`wiki`, the
    /// core's Markdown pages): theirs are not asked.
    pub replaces: Vec<String>,
    /// Its rank among the others when items rank the same (the core's
    /// own are 10, the document's words 0).
    pub priority: i32,
}

/// Asks plugin `spec.plugin` for its completer's items: the plugins'
/// bridge (`crate::extensions`). Nothing while the plugins are busy, or
/// once the keystroke is cancelled.
pub type Provider = dyn Fn(&PluginCompleterSpec, &Context, &Cancel) -> Vec<Item> + Send + Sync;

/// How long a plugin's completer may take for one keystroke when it is
/// asked on a thread: its call's time, and as long again for the plugin
/// to end a call of the drawing's it is in.
pub const PLUGIN_BUDGET: Duration =
    Duration::from_millis(2 * crate::extensions::DRAW_TIME.as_millis() as u64);

struct Registry {
    completers: Vec<Arc<PluginCompleter>>,
    provider: Option<Arc<Provider>>,
    /// Whether the provider is asked on a thread of its own (the plugins'
    /// bridge, [`set_background_provider`]).
    background: bool,
}

static REGISTRY: Mutex<Registry> = Mutex::new(Registry {
    completers: Vec::new(),
    provider: None,
    background: false,
});

fn registry() -> std::sync::MutexGuard<'static, Registry> {
    REGISTRY.lock().unwrap_or_else(|e| e.into_inner())
}

/// Adds a plugin's completer, in place of one of the same plugin and ID.
pub fn register(spec: PluginCompleterSpec) {
    let c = Arc::new(PluginCompleter::new(spec));
    let mut r = registry();
    r.completers
        .retain(|x| !(x.spec.plugin == c.spec.plugin && x.spec.id == c.spec.id));
    r.completers.push(c);
}

/// Removes plugin `plugin`'s completers.
pub fn remove_plugin(plugin: &str) {
    registry().completers.retain(|c| c.spec.plugin != plugin);
}

/// The plugins' completers.
pub fn specs() -> Vec<PluginCompleterSpec> {
    registry()
        .completers
        .iter()
        .map(|c| c.spec.clone())
        .collect()
}

/// Sets what asks the plugins' completers, asked on the editor's thread
/// (a test's).
pub fn set_provider(provider: Arc<Provider>) {
    let mut r = registry();
    r.provider = Some(provider);
    r.background = false;
}

/// Sets the bridge to the plugins (once, as they are installed), asked
/// on a thread of its own for each keystroke: the plugins' completers are
/// slow completers ([`Completer::slow`]).
pub fn set_background_provider(provider: Arc<Provider>) {
    let mut r = registry();
    r.provider = Some(provider);
    r.background = true;
}

/// The plugins' completers, for the registry to run with its own.
pub(super) fn completers() -> Vec<Arc<dyn Completer>> {
    registry()
        .completers
        .iter()
        .map(|c| c.clone() as Arc<dyn Completer>)
        .collect()
}

/// `s` for as long as the program runs: the completers' IDs and trigger
/// strings, which the contract gives as `&'static`; the same string once,
/// however often a plugin starts again.
fn intern(s: &str) -> &'static str {
    static STRINGS: Mutex<Option<HashMap<String, &'static str>>> = Mutex::new(None);
    let mut m = STRINGS.lock().unwrap_or_else(|e| e.into_inner());
    let m = m.get_or_insert_with(HashMap::new);
    if let Some(s) = m.get(s) {
        return s;
    }
    let leaked: &'static str = Box::leak(s.to_string().into_boxed_str());
    m.insert(s.to_string(), leaked);
    leaked
}

/// The strings of `v` interned, as one slice, interned too.
fn intern_all(v: &[String]) -> &'static [&'static str] {
    type Lists = HashMap<Vec<String>, &'static [&'static str]>;
    static LISTS: Mutex<Option<Lists>> = Mutex::new(None);
    let mut m = LISTS.lock().unwrap_or_else(|e| e.into_inner());
    let m = m.get_or_insert_with(HashMap::new);
    if let Some(l) = m.get(v) {
        return l;
    }
    let list: Vec<&'static str> = v.iter().map(|s| intern(s)).collect();
    let leaked: &'static [&'static str] = Box::leak(list.into_boxed_slice());
    m.insert(v.to_vec(), leaked);
    leaked
}

/// A plugin's completer as the core runs one.
struct PluginCompleter {
    spec: PluginCompleterSpec,
    /// `plugin.id`, the items' source.
    name: &'static str,
    trigger: Trigger,
    replaces: &'static [&'static str],
}

impl PluginCompleter {
    fn new(spec: PluginCompleterSpec) -> PluginCompleter {
        let strings = intern_all(&spec.triggers);
        let trigger = match (spec.word, strings.is_empty()) {
            (Some(n), true) => Trigger::WordPrefix(n.max(1)),
            (Some(n), false) => Trigger::WordOrAfter(n.max(1), strings),
            (None, false) => Trigger::Strings(strings),
            (None, true) => Trigger::Request,
        };
        PluginCompleter {
            name: intern(&format!("{}.{}", spec.plugin, spec.id)),
            trigger,
            replaces: intern_all(&spec.replaces),
            spec,
        }
    }
}

impl Completer for PluginCompleter {
    fn id(&self) -> &'static str {
        self.name
    }

    fn priority(&self) -> i32 {
        self.spec.priority
    }

    fn scope(&self) -> crate::command::Scope {
        if self.spec.types.is_empty() {
            crate::command::Scope::all()
        } else {
            let types: Vec<&str> = self.spec.types.iter().map(String::as_str).collect();
            crate::command::Scope::only(&types)
        }
    }

    fn applies(&self, ctx: &Context) -> bool {
        self.spec.layers.is_empty()
            || crate::layers::layer_for(ctx.path.as_deref(), ctx.mode.name())
                .is_some_and(|l| l.plugin == self.spec.plugin && self.spec.layers.contains(&l.id))
    }

    fn trigger(&self) -> Trigger {
        self.trigger
    }

    fn slow(&self) -> bool {
        registry().background
    }

    fn budget(&self) -> Duration {
        PLUGIN_BUDGET
    }

    fn replaces(&self) -> &'static [&'static str] {
        self.replaces
    }

    fn complete(&self, ctx: &Context, _doc: Option<&DocumentState>, cancel: &Cancel) -> Vec<Item> {
        let Some(provider) = registry().provider.clone() else {
            return Vec::new();
        };
        if cancel.cancelled() {
            return Vec::new();
        }
        provider(&self.spec, ctx, cancel)
            .into_iter()
            .filter(|i| sound(i, ctx))
            .collect()
    }
}

/// Whether a plugin's item can be applied: it replaces text of the copy
/// the plugin was given, ending at the cursor, from a character's start,
/// and its cursor is a character's start in what it inserts.
fn sound(item: &Item, ctx: &Context) -> bool {
    let r = &item.range;
    r.end == ctx.point
        && r.start <= r.end
        && r.start >= ctx.base
        && ctx.text.is_char_boundary(r.start - ctx.base)
        && item.cursor <= item.insert.len()
        && item.insert.is_char_boundary(item.cursor)
}
