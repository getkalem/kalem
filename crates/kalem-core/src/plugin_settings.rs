//! The installed plugins in the settings panel: a page listing them, and
//! a page for each with its settings and what can be done with it
//! ([`crate::settings_list`]). A plugin's settings are kept in the user's
//! `settings.toml` under `[plugins."ID"]`; the panel shows
//!
//! - a language plugin's choice of server (`server`: `auto`, one of the
//!   servers its manifest names, or `off`);
//! - the settings its manifest describes under `"settings"`, each with a
//!   `type` (`boolean`, `integer` with `minimum` and `maximum`, `string`
//!   with an `enum` of choices or `examples` to step through, `array` of
//!   strings, with an `enum` in `items` for a list of choices), a
//!   `default` and a `description`, as VS Code's extensions describe
//!   theirs;
//! - any other key the user's table holds, typed as JSON;
//!
//! then updating it, its folder and removing it.

use std::path::PathBuf;

use serde_json::Value;

use crate::l10n::tr;
use crate::settings::Config;
use crate::settings_list::{Entry, Field, FieldKind, Row};

/// An installed plugin, as its page shows it.
#[derive(Debug, Clone, PartialEq)]
pub struct PluginInfo {
    /// Its ID.
    pub id: String,
    /// Its name.
    pub name: String,
    /// Its version.
    pub version: String,
    /// What it does, from its manifest.
    pub description: String,
    /// Its folder.
    pub dir: PathBuf,
    /// Where it was installed from, when Kalem installed it.
    pub source: Option<String>,
    /// The language servers it offers, by key, with their names.
    pub servers: Vec<(String, String)>,
    /// The settings its manifest describes.
    pub settings: Vec<Field>,
    /// Turned off after stopping too often.
    pub turned_off: bool,
}

/// The installed plugins, their manifests read.
pub fn installed() -> Vec<PluginInfo> {
    crate::settings::config_dir().map_or_else(Vec::new, |d| installed_in(&d))
}

/// [`installed`] from the settings folder `config`.
pub fn installed_in(config: &std::path::Path) -> Vec<PluginInfo> {
    crate::plugin_store::installed_in(config)
        .into_iter()
        .map(|p| {
            let manifest = std::fs::read_to_string(p.dir.join("plugin.json"))
                .ok()
                .and_then(|t| serde_json::from_str(&t).ok())
                .unwrap_or(Value::Null);
            let mut info = read(&manifest, p.dir, p.source);
            info.id = p.id;
            info.name = p.name;
            info.version = p.version;
            info.turned_off = crate::plugin_store::turned_off(&info.id, &info.version);
            info
        })
        .collect()
}

/// The plugin in folder `dir` with manifest `m`.
pub fn read(m: &Value, dir: PathBuf, source: Option<String>) -> PluginInfo {
    let text = |k: &str| m[k].as_str().unwrap_or_default().to_string();
    let id = text("id");
    let servers = m["servers"]
        .as_object()
        .map(|o| {
            o.iter()
                .map(|(k, s)| (k.clone(), s["name"].as_str().unwrap_or(k).to_string()))
                .collect()
        })
        .unwrap_or_default();
    PluginInfo {
        settings: declared(&id, m),
        name: m["name"]
            .as_str()
            .map_or_else(|| id.clone(), str::to_string),
        version: text("version"),
        description: text("description"),
        id,
        dir,
        source,
        servers,
        turned_off: false,
    }
}

/// The settings manifest `m` of plugin `id` describes.
fn declared(id: &str, m: &Value) -> Vec<Field> {
    let strings = |v: &Value| -> Vec<String> {
        v.as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|x| x.as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default()
    };
    let Some(settings) = m["settings"].as_object() else {
        return Vec::new();
    };
    settings
        .iter()
        .map(|(key, s)| {
            let kind = match s["type"].as_str() {
                Some("boolean") => FieldKind::Bool,
                Some("integer" | "number") => FieldKind::Int(
                    s["minimum"].as_i64().unwrap_or(i64::MIN),
                    s["maximum"].as_i64().unwrap_or(i64::MAX),
                ),
                Some("string") if s["enum"].is_array() => FieldKind::Enum(strings(&s["enum"])),
                Some("string") => FieldKind::Text(strings(&s["examples"])),
                Some("array") if s["items"]["enum"].is_array() => {
                    FieldKind::Choices(strings(&s["items"]["enum"]))
                }
                Some("array") => FieldKind::Texts,
                _ => FieldKind::Json,
            };
            let default = match (&s["default"], &kind) {
                (Value::Null, FieldKind::Bool) => Value::Bool(false),
                (Value::Null, FieldKind::Text(_)) => Value::from(""),
                (Value::Null, FieldKind::Choices(_) | FieldKind::Texts) => Value::Array(Vec::new()),
                (d, _) => d.clone(),
            };
            let description = s["description"].as_str().unwrap_or_default().to_string();
            Field::plugin(id, key, kind, default, description)
        })
        .collect()
}

/// The page of plugin `p`: its settings (the server, those its manifest
/// describes, the user's other keys), then what can be done with it.
pub fn rows(p: &PluginInfo, config: &Config) -> Vec<Row> {
    let mut out = vec![Row::Heading(format!("{} {}", p.name, p.version))];
    let mut known: Vec<String> = Vec::new();
    if !p.servers.is_empty() {
        let mut options = vec!["auto".to_string()];
        options.extend(p.servers.iter().map(|(k, _)| k.clone()));
        options.push("off".into());
        let names = p
            .servers
            .iter()
            .map(|(k, n)| format!("{k} ({n})"))
            .collect::<Vec<_>>()
            .join(", ");
        out.push(Row::Entry(Entry::Field(Field::plugin(
            &p.id,
            "server",
            FieldKind::Enum(options),
            Value::from("auto"),
            crate::tr!("settings-plugin-server", servers = names),
        ))));
        known.push("server".into());
    }
    for f in &p.settings {
        known.push(f.name.clone());
        out.push(Row::Entry(Entry::Field(f.clone())));
    }
    if let Some(Value::Object(t)) = config.get_path(&["plugins", &p.id]) {
        for k in t.keys().filter(|k| !known.contains(k)) {
            out.push(Row::Entry(Entry::Field(Field::plugin(
                &p.id,
                k,
                FieldKind::Json,
                Value::Null,
                tr("settings-plugin-other"),
            ))));
        }
    }
    out.push(Row::Heading(tr("settings-plugin-actions")));
    if let Some(src) = &p.source {
        out.push(Row::Entry(Entry::Action {
            label: crate::tr!("plugin-update", name = p.name.as_str()),
            about: crate::tr!("plugin-from-short", source = src.as_str()),
            command: "plugin.install".into(),
            args: serde_json::json!({ "source": src }),
        }));
    }
    out.push(Row::Entry(Entry::Action {
        label: tr("plugin-show-folder"),
        about: p.dir.display().to_string(),
        command: "file.open".into(),
        args: serde_json::json!({ "path": p.dir }),
    }));
    out.push(Row::Entry(Entry::Action {
        label: crate::tr!("plugin-remove", name = p.name.as_str()),
        about: tr("plugin-asks-first"),
        command: "plugin.remove".into(),
        args: serde_json::json!({ "id": p.id }),
    }));
    out
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::settings_list::{self, Edit};

    /// A plugin describing a setting of each kind, with a server.
    pub(crate) fn example() -> PluginInfo {
        let m = serde_json::json!({
            "id": "org.example.count",
            "name": "Count",
            "version": "1.2.0",
            "description": "Counts words",
            "servers": {"wc": {"name": "Word Counter"}},
            "settings": {
                "goal": {"type": "integer", "minimum": 0, "maximum": 10000,
                         "default": 1000, "description": "Words a day"},
                "show": {"type": "boolean", "default": true},
                "style": {"type": "string", "enum": ["plain", "bars"], "default": "plain"},
                "folder": {"type": "string", "examples": ["notes", "journal"]},
                "kinds": {"type": "array", "items": {"enum": ["org", "md"]}},
                "skip": {"type": "array"},
                "raw": {"type": "object"}
            }
        });
        let mut p = read(
            &m,
            PathBuf::from("/plugins/count"),
            Some("example/count".into()),
        );
        p.id = "org.example.count".into();
        p
    }

    #[test]
    fn a_manifest_describes_its_settings() {
        let p = example();
        let kind = |k: &str| {
            p.settings
                .iter()
                .find(|f| f.name == k)
                .map(|f| f.kind.clone())
                .unwrap()
        };
        assert_eq!(kind("goal"), FieldKind::Int(0, 10000));
        assert_eq!(kind("show"), FieldKind::Bool);
        assert_eq!(
            kind("style"),
            FieldKind::Enum(vec!["plain".into(), "bars".into()])
        );
        assert_eq!(
            kind("folder"),
            FieldKind::Text(vec!["notes".into(), "journal".into()])
        );
        assert_eq!(
            kind("kinds"),
            FieldKind::Choices(vec!["org".into(), "md".into()])
        );
        assert_eq!(kind("skip"), FieldKind::Texts);
        assert_eq!(kind("raw"), FieldKind::Json);
        let folder = p.settings.iter().find(|f| f.name == "folder").unwrap();
        assert_eq!(folder.path, ["plugins", "org.example.count", "folder"]);
        // A text steps through its examples.
        let config = Config::default();
        assert_eq!(
            settings_list::step(&config, folder, true),
            Some(Value::from("notes"))
        );
    }

    #[test]
    fn a_plugins_page() {
        let p = example();
        let config = Config::from_layers(&[(
            crate::settings::Layer::User,
            None,
            "[plugins.\"org.example.count\"]\nserver = \"off\"\nold = 3\n",
        )]);
        let rows = rows(&p, &config);
        let entries: Vec<&Entry> = rows
            .iter()
            .filter_map(|r| match r {
                Row::Entry(e) => Some(e),
                Row::Heading(_) => None,
            })
            .collect();
        // The server first, its choices the manifest's servers.
        let Entry::Field(server) = entries[0] else {
            panic!("{:?}", entries[0]);
        };
        assert_eq!(server.name, "server");
        assert_eq!(settings_list::shown(&config, server), "off");
        assert_eq!(
            settings_list::step(&config, server, false),
            Some(Value::from("wc"))
        );
        // A key the manifest does not describe, typed as JSON.
        let old = entries
            .iter()
            .find_map(|e| e.field().filter(|f| f.name == "old"))
            .expect("the user's other key");
        assert_eq!(settings_list::edit(old), Edit::Type);
        assert_eq!(settings_list::shown(&config, old), "3");
        // Then what can be done with it.
        let commands: Vec<&str> = entries
            .iter()
            .filter_map(|e| match e {
                Entry::Action { command, .. } => Some(command.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(commands, ["plugin.install", "file.open", "plugin.remove"]);
    }
}
