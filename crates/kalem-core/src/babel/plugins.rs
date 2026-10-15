//! Source blocks run by plugins (plugin API 0.2.21, the `executor`
//! export): each language declared in its plugin's manifest
//! (`"executors": [{"language": "ledger", "headers": ":results output
//! :cmdline bal"}]`) and asked through the plugins' bridge, on the editor's
//! thread, while its plugin runs. [`prepare`] turns a block into the
//! program Babel runs, as it runs its own languages' (a program the
//! manifest names, found as `process.run` finds it), and [`finish`] what
//! the program wrote into the block's value.

use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard};

use super::value::Value;

/// A plugin's language, as its manifest declares it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginExecutorSpec {
    /// The plugin, by its short ID.
    pub plugin: String,
    /// The language, as a source block names it (lower case).
    pub language: String,
    /// Its default header arguments (`org-babel-default-header-args:LANG`).
    pub headers: String,
}

/// A value as a plugin gives and is given it: a variable's, or a result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Data {
    /// Text; a number as Org writes it.
    Text(String),
    /// A plain list's items.
    Items(Vec<String>),
    /// A table's rows; none for a rule.
    Table(Vec<Option<Vec<String>>>),
    /// What a program wrote, read as Org reads a shell's.
    Output(String),
}

impl Data {
    /// `v`, a variable's value, as a plugin is given it: a list of rows a
    /// table, a list of atoms a list, else text as `princ` writes it.
    pub fn of_value(v: &Value) -> Data {
        match v {
            Value::List(items)
                if items
                    .iter()
                    .any(|i| matches!(i, Value::List(_)) || i.is_hline()) =>
            {
                Data::Table(
                    items
                        .iter()
                        .map(|row| match row {
                            Value::List(cells) => Some(cells.iter().map(Value::princ).collect()),
                            h if h.is_hline() => None,
                            o => Some(vec![o.princ()]),
                        })
                        .collect(),
                )
            }
            Value::List(items) if !items.is_empty() => {
                Data::Items(items.iter().map(Value::princ).collect())
            }
            o => Data::Text(o.princ()),
        }
    }

    /// The block's value for its `:results` words `results`: output read
    /// as `org-babel-result-cond` reads a shell's.
    pub fn into_value(self, results: &[String]) -> Value {
        match self {
            Data::Text(t) => Value::Str(t),
            Data::Items(items) => Value::List(items.into_iter().map(Value::Str).collect()),
            Data::Table(rows) => Value::List(
                rows.into_iter()
                    .map(|r| match r {
                        Some(cells) => Value::List(cells.into_iter().map(Value::Str).collect()),
                        None => Value::Symbol("hline".into()),
                    })
                    .collect(),
            ),
            Data::Output(t) => {
                if super::scalar(results) {
                    Value::Str(t)
                } else {
                    super::table::import(&t)
                }
            }
        }
    }
}

/// A block as a plugin is given it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Block {
    pub language: String,
    pub body: String,
    /// Its header arguments merged, `:results` and the `:var`s apart.
    pub headers: Vec<(String, String)>,
    /// `:results`' words.
    pub results: Vec<String>,
    pub vars: Vec<(String, Data)>,
    /// Its document's file.
    pub path: Option<PathBuf>,
    /// The folder it runs in.
    pub dir: PathBuf,
}

/// What a block comes to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Prepared {
    /// A program to run, found and granted.
    Run(crate::extensions::ProcessRequest),
    /// Its value at once.
    Done(Data),
}

/// What the plugin is asked.
#[derive(Debug, Clone, Copy)]
pub enum Call<'a> {
    /// What running the block takes.
    Prepare(&'a Block),
    /// The block's value from its program's output and exit status.
    Finish(&'a Block, &'a str, Option<i32>),
}

/// What it answered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Answer {
    /// To [`Call::Prepare`].
    Prepared(Prepared),
    /// To [`Call::Finish`].
    Value(Data),
    /// Why not: the plugin's refusal, or the program it may not run.
    Refused(String),
    /// The plugin was busy with another call: not asked.
    Busy,
    /// The plugin is gone, or failed.
    Failed(String),
}

/// Asks plugin `spec.plugin`: the plugins' bridge (`crate::extensions`).
pub type Provider = dyn Fn(&PluginExecutorSpec, Call<'_>) -> Answer + Send + Sync;

struct Registry {
    executors: Vec<PluginExecutorSpec>,
    provider: Option<Arc<Provider>>,
}

static REGISTRY: Mutex<Registry> = Mutex::new(Registry {
    executors: Vec::new(),
    provider: None,
});

fn registry() -> MutexGuard<'static, Registry> {
    REGISTRY.lock().unwrap_or_else(|e| e.into_inner())
}

/// Adds a plugin's language, in place of one of the same plugin and
/// language.
pub fn register(spec: PluginExecutorSpec) {
    let mut r = registry();
    r.executors
        .retain(|e| !(e.plugin == spec.plugin && e.language == spec.language));
    r.executors.push(spec);
}

/// Takes plugin `plugin`'s languages away (it stopped).
pub fn remove_plugin(plugin: &str) {
    registry().executors.retain(|e| e.plugin != plugin);
}

/// The bridge the plugins are asked through.
pub fn set_provider(provider: Arc<Provider>) {
    registry().provider = Some(provider);
}

/// The plugin language `language` (a block's), the first registered.
pub fn for_language(language: &str) -> Option<PluginExecutorSpec> {
    let l = language.to_lowercase();
    registry()
        .executors
        .iter()
        .find(|e| e.language == l)
        .cloned()
}

/// Asks `spec`'s plugin `call`.
pub fn ask(spec: &PluginExecutorSpec, call: Call<'_>) -> Answer {
    let Some(provider) = registry().provider.clone() else {
        return Answer::Failed(crate::tr!(
            "babel-plugin-gone",
            plugin = spec.plugin.as_str()
        ));
    };
    provider(spec, call)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn found_by_language() {
        let spec = |headers: &str| PluginExecutorSpec {
            plugin: "extest".into(),
            language: "extestlang".into(),
            headers: headers.into(),
        };
        register(spec(""));
        // The same plugin and language once.
        register(spec(":cmdline bal"));
        assert_eq!(
            for_language("EXTESTLANG").map(|s| s.headers),
            Some(":cmdline bal".into())
        );
        assert!(for_language("python").is_none());
        remove_plugin("extest");
        assert!(for_language("extestlang").is_none());
    }

    #[test]
    fn values_both_ways() {
        let table = Value::List(vec![
            Value::List(vec![Value::Str("a".into()), Value::Int(1)]),
            Value::Symbol("hline".into()),
        ]);
        assert_eq!(
            Data::of_value(&table),
            Data::Table(vec![Some(vec!["a".into(), "1".into()]), None])
        );
        assert_eq!(
            Data::of_value(&Value::List(vec![Value::Str("x".into())])),
            Data::Items(vec!["x".into()])
        );
        assert_eq!(Data::of_value(&Value::Float(2.5)), Data::Text("2.5".into()));
        // Output: text where `:results` asks for text, else a table.
        let output = || Data::Output("a 1\nb 2\n".into());
        assert_eq!(
            output().into_value(&["replace".into(), "output".into()]),
            Value::Str("a 1\nb 2\n".into())
        );
        assert!(matches!(
            output().into_value(&["replace".into()]),
            Value::List(rows) if rows.len() == 2
        ));
        assert_eq!(
            Data::Table(vec![None]).into_value(&[]),
            Value::List(vec![Value::Symbol("hline".into())])
        );
    }
}
