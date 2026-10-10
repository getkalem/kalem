//! The installed extension plugins, loaded into the editors (T3.1.12): the
//! components of the `extension` world, run in `kalem-script`'s host and
//! offered to the editors as `kalem_core::extensions::Extensions`.
//!
//! A plugin's manifest (`plugin.json`) names its component (`main`) and
//! when it starts (`activation`): `onStartup`, `onEvent:NAME` for the
//! first event of that name (`onEvent:document:open`), or `onFile` for
//! the first file opened that the plugin serves, as it declares it
//! ([`kalem_core::applies`], what the index says of it too). With none,
//! a plugin that declares files it serves starts with the first of them,
//! any other with Kalem.
//! It is known by the last part of its ID (`org.kalem.wordcount` is
//! `wordcount`, its commands `wordcount.*`). A plugin that fails (a trap,
//! its time or memory spent) is stopped, what it registered taken back,
//! and the user told.

use std::path::PathBuf;
use std::sync::Arc;

use kalem_core::command::{Command, CommandHandler, CommandSource, Scope};
use kalem_core::events::Event;
use kalem_core::keys::KeySequence;
use kalem_core::when::WhenClause;
use kalem_script::extension::{self as x, Editor, Extension, Question};
use kalem_script::{Host, Limits};

/// The editor as a plugin reaches it: `kalem_core::extensions` and the
/// background notices.
struct Bridge {
    /// The plugin's manifest ID, its settings' table.
    id: String,
}

impl Editor for Bridge {
    fn add_command(&mut self, plugin: &str, spec: &x::CommandSpec) -> Result<(), String> {
        let mut default_keys = Vec::new();
        for k in &spec.keys {
            default_keys.push(KeySequence::parse(k).ok_or_else(|| format!("`{k}` are not keys"))?);
        }
        let when = spec
            .when
            .as_deref()
            .map(WhenClause::parse)
            .transpose()
            .map_err(|e| format!("`{}`: {e:?}", spec.when.as_deref().unwrap_or_default()))?;
        let args_schema = spec
            .args_schema
            .as_deref()
            .map(serde_json::from_str)
            .transpose()
            .map_err(|e| format!("The arguments' schema: {e}"))?;
        kalem_core::extensions::add_command(Command {
            id: spec.id.clone(),
            title: spec.title.clone(),
            category: spec.category.clone(),
            default_keys,
            when,
            handler: CommandHandler::Plugin(plugin.to_string()),
            args_schema,
            source: CommandSource::Plugin(plugin.to_string()),
            scope: Some(Scope {
                types: spec.scope.types.clone(),
                except: spec.scope.except.clone(),
            }),
        })
    }

    fn remove_command(&mut self, id: &str) {
        kalem_core::extensions::remove_command(id);
    }

    fn add_binding(
        &mut self,
        id: u64,
        keys: &str,
        command: &str,
        when: Option<&str>,
    ) -> Result<(), String> {
        kalem_core::extensions::add_binding(binding_id(id), keys, command, when)
    }

    fn remove_binding(&mut self, id: u64) {
        kalem_core::extensions::remove_binding(binding_id(id));
    }

    fn run(&mut self, id: &str, args: &str) -> Result<(), String> {
        if !kalem_core::extensions::known(id) {
            return Err(format!("Unknown command `{id}`"));
        }
        let args = serde_json::from_str(args).map_err(|e| format!("The arguments: {e}"))?;
        kalem_core::extensions::queue_run(id, args);
        Ok(())
    }

    fn notify(&mut self, plugin: &str, message: &str, level: x::Level) {
        kalem_core::jobs::notice(
            format!("{plugin}: {message}"),
            matches!(level, x::Level::Error),
        );
    }

    fn ask(&mut self, _plugin: &str, request: u64, question: Question) {
        use kalem_core::extensions::Question as Q;
        kalem_core::extensions::ask(
            request,
            match question {
                Question::Prompt { title, options } => Q::Prompt {
                    title,
                    value: options.value,
                },
                Question::Confirm(message) => Q::Confirm(message),
                Question::Pick { items, options } => Q::Pick {
                    title: options.title,
                    items: items.into_iter().map(|i| (i.label, i.detail)).collect(),
                },
            },
        );
    }

    fn withdraw(&mut self, request: u64) {
        kalem_core::extensions::withdraw(request);
    }

    fn set_status(&mut self, plugin: &str, id: &str, text: &str, o: &x::StatusOptions) {
        kalem_core::extensions::set_status(kalem_core::extensions::StatusItem {
            plugin: plugin.to_string(),
            id: id.to_string(),
            text: text.to_string(),
            tooltip: o.tooltip.clone(),
            command: o.command.clone(),
            right: matches!(o.alignment, x::ui::Alignment::Right),
            priority: o.priority,
        });
    }

    fn remove_status(&mut self, plugin: &str, id: &str) {
        kalem_core::extensions::remove_status(plugin, id);
    }

    fn add_panel(&mut self, plugin: &str, spec: &x::PanelSpec) -> Result<(), String> {
        kalem_core::extensions::add_panel(kalem_core::extensions::Panel {
            plugin: plugin.to_string(),
            id: spec.id.clone(),
            title: spec.title.clone(),
            bottom: matches!(spec.placement, x::ui::Placement::Bottom),
            widgets: Vec::new(),
        });
        Ok(())
    }

    fn set_panel(&mut self, id: &str, tree: &x::WidgetTree) {
        kalem_core::extensions::set_panel(id, tree.widgets.iter().map(widget).collect());
    }

    fn remove_panel(&mut self, id: &str) {
        kalem_core::extensions::remove_panel(id);
    }

    fn setting(&mut self, key: &str) -> Option<String> {
        let parts: Vec<&str> = key.split('.').collect();
        kalem_core::extensions::setting(&parts).map(|v| v.to_string())
    }

    fn own_setting(&mut self, _plugin: &str, key: &str) -> Option<String> {
        kalem_core::extensions::setting(&["plugins", &self.id, key]).map(|v| v.to_string())
    }

    fn set_own_setting(&mut self, _plugin: &str, key: &str, value: &str) -> Result<(), String> {
        let value: serde_json::Value =
            serde_json::from_str(value).map_err(|e| format!("The value: {e}"))?;
        kalem_core::extensions::set_own_setting(&self.id, key, &value)
    }

    fn workspace(&mut self) -> Vec<PathBuf> {
        kalem_core::extensions::workspace()
    }

    fn refresh_layers(&mut self, _plugin: &str) {
        kalem_core::layers::refresh();
    }

    fn document(&mut self) -> Option<Box<dyn x::DocumentAccess + '_>> {
        kalem_core::plugin_doc::with_document(|_| ())?;
        Some(Box::new(Document))
    }

    fn fetch(&mut self, _plugin: &str, id: u64, request: x::http::Request) {
        let _ = std::thread::Builder::new()
            .name("kalem-plugin-fetch".into())
            .spawn(move || {
                let response = fetch(&request);
                kalem_core::extensions::respond(id, response);
            });
    }

    fn spawn(&mut self, _plugin: &str, run: u64, cwd: PathBuf, command: x::process::Command) {
        use kalem_core::extensions as e;
        let Some(program) = e::find_program(&self.id, &command.program, &cwd) else {
            return e::fail_run(
                run,
                format!(
                    "{name} was not found: install it, or give its path as the setting \
                     programs.{name} of the plugin",
                    name = command.program
                ),
            );
        };
        e::start_run(
            run,
            e::ProcessRequest {
                program,
                args: command.args,
                cwd,
                stdin: command.stdin,
                env: command.env,
            },
        );
    }

    fn kill(&mut self, run: u64) {
        kalem_core::extensions::kill_run(run);
    }

    fn open_document(
        &mut self,
        plugin: &str,
        spec: x::documents::DocumentSpec,
        text: String,
        cursor: Option<u64>,
        styles: Vec<x::styled_documents::StyledSpan>,
    ) -> Result<u64, String> {
        kalem_core::extensions::open_generated(
            plugin,
            kalem_core::extensions::GeneratedSpec {
                id: spec.id,
                key: spec.key,
                title: spec.title,
                kind: spec.kind,
                language: spec.language,
            },
            text,
            cursor.map(|c| c as usize),
            core_styles(styles),
        )
    }

    fn set_document(
        &mut self,
        plugin: &str,
        doc: u64,
        text: String,
        cursor: Option<u64>,
        styles: Vec<x::styled_documents::StyledSpan>,
    ) -> Result<(), String> {
        kalem_core::extensions::set_generated(
            plugin,
            doc,
            text,
            cursor.map(|c| c as usize),
            core_styles(styles),
        )
    }

    fn close_document(&mut self, plugin: &str, doc: u64) {
        kalem_core::extensions::close_generated(plugin, doc);
    }

    fn set_gutter(
        &mut self,
        plugin: &str,
        path: PathBuf,
        marks: Vec<x::decorations::LineMark>,
    ) -> Result<(), String> {
        use kalem_core::GutterMark as M;
        use x::decorations::MarkKind as K;
        let marks = marks
            .into_iter()
            .map(|m| {
                let kind = match m.kind {
                    K::Added => M::Added,
                    K::Changed => M::Changed,
                    K::Removed => M::Removed,
                };
                (m.line, kind)
            })
            .collect();
        kalem_core::extensions::set_gutter(plugin, &path, marks)
    }

    fn clear_gutter(&mut self, plugin: &str, path: Option<PathBuf>) {
        kalem_core::extensions::clear_gutter(plugin, path.as_deref());
    }
}

/// A plugin's styled stretches in the core's terms.
fn core_styles(styles: Vec<x::styled_documents::StyledSpan>) -> Vec<kalem_core::StyleSpan> {
    use kalem_core::StyleColor as C;
    use x::styled_documents::Color as W;
    styles
        .into_iter()
        .map(|s| kalem_core::StyleSpan {
            range: s.start as usize..s.end as usize,
            style: kalem_core::SpanStyle {
                color: match s.style.color {
                    W::Default => C::Default,
                    W::Muted => C::Muted,
                    W::Red => C::Red,
                    W::Green => C::Green,
                    W::Yellow => C::Yellow,
                    W::Blue => C::Blue,
                    W::Magenta => C::Magenta,
                    W::Cyan => C::Cyan,
                    W::Accent => C::Accent,
                },
                bold: s.style.bold,
                italic: s.style.italic,
                underline: s.style.underline,
            },
        })
        .collect()
}

/// The document of the plugin's command running on this thread
/// (`kalem_core::plugin_doc`), in the API's terms.
struct Document;

fn read<R: Default>(f: impl FnOnce(&kalem_core::plugin_doc::DocView) -> R) -> R {
    kalem_core::plugin_doc::with_document(f).unwrap_or_default()
}

fn range(start: usize, end: usize) -> x::api::Range {
    x::api::Range {
        start: start as u64,
        end: end as u64,
    }
}

impl x::DocumentAccess for Document {
    fn info(&self) -> x::editor::DocumentInfo {
        read(|d| {
            Some(x::editor::DocumentInfo {
                path: d.path.clone(),
                mode: d.mode.clone(),
                language: d.language.clone(),
                modified: d.modified,
                length: d.text.len() as u64,
            })
        })
        .unwrap_or(x::editor::DocumentInfo {
            path: None,
            mode: String::new(),
            language: None,
            modified: false,
            length: 0,
        })
    }

    fn generated(&self) -> Option<u64> {
        read(|d| d.generated)
    }

    fn selection(&self) -> x::editor::Selection {
        let (anchor, head) = read(|d| d.selection);
        x::editor::Selection {
            anchor: anchor as u64,
            head: head as u64,
        }
    }

    fn text(&self, r: Option<(u64, u64)>) -> String {
        read(|d| d.text(r.map(|(s, e)| (s as usize, e as usize))))
    }

    fn headlines(&self) -> Vec<x::editor::Headline> {
        read(|d| d.headlines())
            .into_iter()
            .map(|h| x::editor::Headline {
                start: h.start as u64,
                range: range(h.start, h.end),
                level: h.level.min(255) as u8,
                title: h.title,
                todo: h.todo,
                done: h.done,
                priority: h.priority.map(String::from),
                tags: h.tags,
                properties: h.properties,
                scheduled: h.scheduled,
                deadline: h.deadline,
                closed: h.closed,
                parent: h.parent.map(|p| p as u64),
                children: h.children.into_iter().map(|c| c as u64).collect(),
            })
            .collect()
    }

    fn body(&self, start: u64) -> Option<String> {
        read(|d| d.body(start as usize))
    }

    fn todo_keywords(&self) -> Vec<String> {
        read(|d| d.todo_keywords())
    }

    fn keywords(&self) -> Vec<(String, String)> {
        read(|d| d.keywords())
    }

    fn node_at(&self, offset: u64) -> Option<x::editor::Node> {
        read(|d| d.node_at(offset as usize)).map(|(kind, s, e)| x::editor::Node {
            kind,
            range: range(s, e),
        })
    }

    fn table_at(&self, offset: u64) -> Option<x::editor::Table> {
        read(|d| d.table_at(offset as usize)).map(|t| x::editor::Table {
            start: t.start as u64,
            range: range(t.start, t.end),
            rows: t.rows,
            formulas: t.formulas,
        })
    }

    fn edit(&mut self, edit: x::Edit) {
        use kalem_core::plugin_doc::DocEdit as D;
        let at = |p: u64| p as usize;
        kalem_core::plugin_doc::queue(match edit {
            x::Edit::Insert { at: p, text } => D::Insert(p.map(at), text),
            x::Edit::Replace { start, end, text } => D::Replace(at(start), at(end), text),
            x::Edit::SetTodo { headline, state } => D::SetTodo(at(headline), state),
            x::Edit::SetTitle { headline, title } => D::SetTitle(at(headline), title),
            x::Edit::SetTags { headline, tags } => D::SetTags(at(headline), tags),
            x::Edit::SetProperty {
                headline,
                key,
                value,
            } => D::SetProperty(at(headline), key, value),
            x::Edit::Promote(h) => D::Promote(at(h)),
            x::Edit::Demote(h) => D::Demote(at(h)),
            x::Edit::MoveUp(h) => D::MoveUp(at(h)),
            x::Edit::MoveDown(h) => D::MoveDown(at(h)),
            x::Edit::SetCell {
                table,
                row,
                col,
                value,
            } => D::SetCell(at(table), row as usize, col as usize, value),
            x::Edit::Recalc(t) => D::Recalc(at(t)),
            x::Edit::Label(l) => D::Label(l),
            x::Edit::Save => D::Save,
        });
    }
}

/// Sends a plugin's request, its 4xx and 5xx answers too; the body
/// at most [`x::MAX_BYTES`].
fn fetch(r: &x::http::Request) -> Result<kalem_core::extensions::HttpResponse, String> {
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .http_status_as_error(false)
        .timeout_global(Some(std::time::Duration::from_secs(60)))
        .build()
        .into();
    let mut builder = ureq::http::Request::builder()
        .method(r.method.as_str())
        .uri(r.url.as_str())
        .header("User-Agent", concat!("Kalem/", env!("CARGO_PKG_VERSION")));
    for h in &r.headers {
        builder = builder.header(h.name.as_str(), h.value.as_str());
    }
    let request = builder
        .body(r.body.clone().unwrap_or_default())
        .map_err(|e| e.to_string())?;
    let mut response = agent.run(request).map_err(|e| e.to_string())?;
    let status = response.status().as_u16();
    let headers = response
        .headers()
        .iter()
        .map(|(n, v)| {
            (
                n.to_string(),
                String::from_utf8_lossy(v.as_bytes()).into_owned(),
            )
        })
        .collect();
    let body = response
        .body_mut()
        .with_config()
        .limit(x::MAX_BYTES as u64)
        .read_to_vec()
        .map_err(|e| e.to_string())?;
    Ok(kalem_core::extensions::HttpResponse {
        status,
        headers,
        body,
    })
}

/// A widget in the core's terms.
fn widget(w: &x::ui::Widget) -> kalem_core::extensions::Widget {
    use kalem_core::extensions::{TextStyle as S, WidgetKind as K};
    use x::ui::{TextStyle, WidgetKind};
    let kind = match &w.kind {
        WidgetKind::Column => K::Column,
        WidgetKind::Row => K::Row,
        WidgetKind::Label(l) => K::Label {
            text: l.text.clone(),
            style: match l.style {
                TextStyle::Normal => S::Normal,
                TextStyle::Strong => S::Strong,
                TextStyle::Emphasis => S::Emphasis,
                TextStyle::Muted => S::Muted,
                TextStyle::Code => S::Code,
                TextStyle::Error => S::Error,
                TextStyle::Heading => S::Heading,
            },
        },
        WidgetKind::Button(b) => K::Button {
            label: b.label.clone(),
            command: b.command.clone(),
        },
        WidgetKind::Input(i) => K::Input {
            value: i.value.clone(),
            placeholder: i.placeholder.clone(),
        },
        WidgetKind::Checkbox(c) => K::Checkbox {
            label: c.label.clone(),
            checked: c.checked,
        },
        WidgetKind::Item(i) => K::Item {
            label: i.label.clone(),
            detail: i.detail.clone(),
            expanded: i.expanded,
            selected: i.selected,
        },
        WidgetKind::Progress(p) => K::Progress {
            value: p.value,
            label: p.label.clone(),
        },
        WidgetKind::Separator => K::Separator,
    };
    kalem_core::extensions::Widget {
        key: w.key.clone(),
        kind,
        children: w.children.clone(),
    }
}

/// Binding numbers are each plugin's own: they are told apart by the
/// plugin's place in the list, in the high bits.
fn binding_id(id: u64) -> u64 {
    PLUGIN.with(|p| (p.get() << 40) | id)
}

thread_local! {
    /// The place of the plugin being called, for [`binding_id`].
    static PLUGIN: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}

/// An installed extension plugin.
struct Loaded {
    /// Its short ID, its commands' prefix.
    id: String,
    /// Its manifest's ID.
    full: String,
    /// What its manifest permits.
    grants: x::Grants,
    file: PathBuf,
    activation: Vec<String>,
    /// When it serves a file, for `onFile`.
    applies: kalem_core::applies::Applies,
    limits: Limits,
    /// Running, once activated.
    extension: Option<Extension>,
    /// Stopped after a failure; not started again until Kalem restarts.
    failed: bool,
    /// The menus its manifest adds to the menu bar, once it runs.
    menus: Vec<kalem_core::extensions::PluginMenu>,
    /// The buttons its manifest adds to the toolbar, once it runs.
    buttons: Vec<kalem_core::extensions::PluginButton>,
    /// The layers its manifest declares, once it runs (API 0.2.10).
    layers: Vec<kalem_core::layers::LayerSpec>,
}

/// A manifest's `layers` for plugin `id` (its short ID): each an ID, the
/// markers a document's folder or one above it must hold, and the modes
/// it serves; one without an ID, markers or modes is left out.
fn manifest_layers(id: &str, m: &serde_json::Value) -> Vec<kalem_core::layers::LayerSpec> {
    let strings = |v: &serde_json::Value| -> Vec<String> {
        v.as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|e| e.as_str().map(str::to_string))
                    .filter(|s| !s.trim().is_empty() && !s.contains(".."))
                    .collect()
            })
            .unwrap_or_default()
    };
    m["layers"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|l| {
                    let spec = kalem_core::layers::LayerSpec {
                        plugin: id.to_string(),
                        id: l["id"].as_str()?.to_string(),
                        markers: strings(&l["markers"]),
                        modes: strings(&l["modes"]),
                    };
                    (!spec.markers.is_empty() && !spec.modes.is_empty()).then_some(spec)
                })
                .collect()
        })
        .unwrap_or_default()
}

/// A layer's overlays as the core's.
fn overlays(set: x::layer::OverlaySet) -> kalem_core::layers::Overlays {
    use kalem_core::layers::{LineEffect, Lines, Overlays, Span, SpanEffect};
    use x::layer::SpanStyle as F;
    let style = |f: F| kalem_core::view::Style {
        bold: f.contains(F::BOLD),
        italic: f.contains(F::ITALIC),
        underline: f.contains(F::UNDERLINE),
        strike: f.contains(F::STRIKE),
        code: f.contains(F::CODE),
        link: f.contains(F::LINK),
        dim: f.contains(F::DIM),
        tag: f.contains(F::TAG),
        todo: if f.contains(F::DONE) {
            Some(true)
        } else if f.contains(F::TODO) {
            Some(false)
        } else {
            None
        },
        timestamp: f.contains(F::TIMESTAMP),
        priority: f.contains(F::PRIORITY),
        ..kalem_core::view::Style::default()
    };
    Overlays {
        spans: set
            .spans
            .into_iter()
            .map(|s| Span {
                range: s.start as usize..s.end as usize,
                effect: match s.effect {
                    x::layer::SpanEffect::Hide => SpanEffect::Hide,
                    x::layer::SpanEffect::Replace(r) => SpanEffect::Replace {
                        text: r.text,
                        style: style(r.style),
                    },
                    x::layer::SpanEffect::Style(f) => SpanEffect::Style(style(f)),
                },
            })
            .collect(),
        lines: set
            .lines
            .into_iter()
            .map(|l| Lines {
                range: l.start as usize..l.end as usize,
                effect: match l.effect {
                    x::layer::LineEffect::Hidden => LineEffect::Hidden,
                    x::layer::LineEffect::Folded => LineEffect::Folded,
                },
            })
            .collect(),
    }
}

/// A manifest's `menus` for plugin `id` (its short ID): each a title, a
/// when-clause on the document (`vcs == git`) and items, the plugin's own
/// commands and `-` between groups. A menu whose when-clause does not
/// parse is left out.
fn manifest_menus(id: &str, m: &serde_json::Value) -> Vec<kalem_core::extensions::PluginMenu> {
    let own = format!("{id}.");
    m["menus"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|menu| {
            let title = menu["title"].as_str()?.trim().to_string();
            if title.is_empty() {
                return None;
            }
            let when = match menu["when"].as_str() {
                Some(w) => Some(WhenClause::parse(w).ok()?),
                None => None,
            };
            let items = menu["items"]
                .as_array()?
                .iter()
                .filter_map(|i| i.as_str())
                .filter(|i| *i == "-" || i.starts_with(&own))
                .map(str::to_string)
                .collect();
            Some(kalem_core::extensions::PluginMenu {
                plugin: id.to_string(),
                title,
                when,
                items,
            })
        })
        .collect()
}

/// A manifest's `buttons` for plugin `id` (its short ID): each a title,
/// one of the plugin's own commands and a when-clause on the document
/// (`vcs == git`). A button whose when-clause does not parse, or whose
/// command is another's, is left out.
fn manifest_buttons(id: &str, m: &serde_json::Value) -> Vec<kalem_core::extensions::PluginButton> {
    let own = format!("{id}.");
    m["buttons"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|button| {
            let title = button["title"].as_str()?.trim().to_string();
            let command = button["command"].as_str()?.to_string();
            if title.is_empty() || !command.starts_with(&own) {
                return None;
            }
            let when = match button["when"].as_str() {
                Some(w) => Some(WhenClause::parse(w).ok()?),
                None => None,
            };
            Some(kalem_core::extensions::PluginButton {
                plugin: id.to_string(),
                title,
                command,
                when,
            })
        })
        .collect()
}

/// The installed extension plugins.
struct Plugins {
    host: Arc<Host>,
    list: Vec<Loaded>,
}

impl Plugins {
    /// Starts plugin `i`: loaded (from the cache when compiled before),
    /// instantiated over the bridge, and activated.
    fn activate(&mut self, i: usize) {
        PLUGIN.with(|p| p.set(i as u64 + 1));
        let host = self.host.clone();
        let l = &mut self.list[i];
        let started = host
            .load_file(&l.file)
            .and_then(|plugin| {
                let bridge = Box::new(Bridge { id: l.full.clone() });
                Extension::new(&host, &plugin, &l.id, bridge, l.grants.clone(), l.limits)
            })
            .map_err(|e| e.to_string())
            .and_then(|mut ext| match ext.activate() {
                Ok(Ok(())) => Ok(ext),
                Ok(Err(e)) => {
                    let _ = ext.deactivate();
                    Err(e)
                }
                Err(e) => {
                    let _ = ext.deactivate();
                    Err(e.to_string())
                }
            });
        match started {
            Ok(ext) => {
                l.extension = Some(ext);
                for m in l.menus.clone() {
                    kalem_core::extensions::add_menu(m);
                }
                for b in l.buttons.clone() {
                    kalem_core::extensions::add_button(b);
                }
                if l.extension.as_ref().is_some_and(Extension::has_layers) {
                    for layer in l.layers.clone() {
                        kalem_core::layers::register(layer);
                    }
                }
            }
            Err(e) => {
                l.failed = true;
                kalem_core::jobs::notice(format!("The plugin {} did not start: {e}", l.id), true);
            }
        }
    }

    /// Stops plugin `i` after `error`: what it registered is taken back.
    fn fail(&mut self, i: usize, error: &kalem_script::Error) {
        PLUGIN.with(|p| p.set(i as u64 + 1));
        let l = &mut self.list[i];
        if let Some(mut ext) = l.extension.take() {
            let _ = ext.deactivate();
        }
        l.failed = true;
        kalem_core::extensions::close_plugin_documents(&l.id);
        kalem_core::extensions::clear_gutter(&l.id, None);
        kalem_core::extensions::remove_menus(&l.id);
        kalem_core::extensions::remove_buttons(&l.id);
        kalem_core::layers::remove_plugin(&l.id);
        kalem_core::jobs::notice(format!("The plugin {} was stopped: {error}", l.id), true);
    }
}

impl kalem_core::extensions::Extensions for Plugins {
    fn overlays(
        &mut self,
        plugin: &str,
        layer: &str,
        path: Option<&std::path::Path>,
        text: &str,
    ) -> kalem_core::layers::Outcome {
        use kalem_core::layers::Outcome;
        let Some(i) = self
            .list
            .iter()
            .position(|l| l.id == plugin && l.extension.is_some())
        else {
            return Outcome::Failed;
        };
        PLUGIN.with(|p| p.set(i as u64 + 1));
        let path = path.map(|p| p.to_string_lossy().into_owned());
        let Some(ext) = self.list[i].extension.as_mut() else {
            return Outcome::Failed;
        };
        match ext.overlays(layer, path.as_deref(), text) {
            Ok(Some(set)) => Outcome::Done(overlays(set)),
            Ok(None) => Outcome::Failed,
            Err(e) => {
                self.fail(i, &e);
                Outcome::Failed
            }
        }
    }

    fn run(&mut self, id: &str, args: &str) -> Result<(), String> {
        let Some(i) = self.list.iter().position(|l| {
            l.extension
                .as_ref()
                .is_some_and(|e| e.commands().contains(id))
        }) else {
            return Err(format!("No plugin runs `{id}`"));
        };
        PLUGIN.with(|p| p.set(i as u64 + 1));
        let Some(ext) = self.list[i].extension.as_mut() else {
            return Err(format!("No plugin runs `{id}`"));
        };
        match ext.run_command(id, args) {
            Ok(Ok(_)) => Ok(()),
            Ok(Err(e)) => Err(e),
            Err(e) => {
                let message = e.to_string();
                self.fail(i, &e);
                Err(message)
            }
        }
    }

    fn event(&mut self, event: &Event) -> Option<String> {
        let name = event.kind().name();
        let waiting = format!("onEvent:{name}");
        // A file opened: the plugins waiting for one they serve.
        let opened = match event {
            Event::DocumentOpen { path: Some(p), .. } => Some(p.as_path()),
            _ => None,
        };
        for i in 0..self.list.len() {
            let l = &self.list[i];
            if l.extension.is_some() || l.failed {
                continue;
            }
            let serves = || {
                l.activation.iter().any(|a| a == "onFile")
                    && opened.is_some_and(|p| l.applies.serves_file(p).is_some())
            };
            if l.activation.contains(&waiting) || serves() {
                self.activate(i);
            }
        }
        if self.list.iter().all(|l| l.extension.is_none()) {
            return None;
        }
        let wit = to_wit(event);
        let kind = x::kind(&wit);
        let mut veto = None;
        for i in 0..self.list.len() {
            let Some(ext) = self.list[i].extension.as_mut() else {
                continue;
            };
            if !ext.wants(kind) {
                continue;
            }
            PLUGIN.with(|p| p.set(i as u64 + 1));
            match ext.event(&wit) {
                Ok(x::Reply::Veto(why)) if veto.is_none() && event.kind().vetoable() => {
                    veto = Some(why);
                }
                Ok(_) => {}
                Err(e) => self.fail(i, &e),
            }
        }
        veto
    }

    fn answer(&mut self, request: u64, answer: kalem_core::extensions::Answer) {
        use kalem_core::extensions::Answer as A;
        let answer = match answer {
            A::Text(t) => x::Answer::Text(t),
            A::Confirmed(yes) => x::Answer::Confirmed(yes),
            A::Picked(p) => x::Answer::Picked(p),
        };
        // The plugin that asked it is the one that takes it.
        for i in 0..self.list.len() {
            PLUGIN.with(|p| p.set(i as u64 + 1));
            let Some(ext) = self.list[i].extension.as_mut() else {
                continue;
            };
            match ext.answer(request, answer.clone()) {
                Ok(true) => return,
                Ok(false) => {}
                Err(e) => return self.fail(i, &e),
            }
        }
    }

    fn settings_changed(&mut self, keys: &[String]) {
        for i in 0..self.list.len() {
            PLUGIN.with(|p| p.set(i as u64 + 1));
            let table = format!("plugins.{}", self.list[i].full);
            let Some(ext) = self.list[i].extension.as_mut() else {
                continue;
            };
            for (key, own) in ext.watches() {
                let full = if own {
                    format!("{table}.{key}")
                } else {
                    key.clone()
                };
                // The key, or a table holding it that came or went whole.
                let changed = keys
                    .iter()
                    .any(|k| full == *k || full.starts_with(&format!("{k}.")));
                if changed && let Err(e) = ext.setting_changed(&key, own) {
                    self.fail(i, &e);
                    break;
                }
            }
        }
    }

    fn respond(&mut self, id: u64, response: Result<kalem_core::extensions::HttpResponse, String>) {
        let response = response.map(|r| x::http::Response {
            status: r.status,
            headers: r
                .headers
                .into_iter()
                .map(|(name, value)| x::http::Header { name, value })
                .collect(),
            body: r.body,
        });
        for i in 0..self.list.len() {
            PLUGIN.with(|p| p.set(i as u64 + 1));
            let Some(ext) = self.list[i].extension.as_mut() else {
                continue;
            };
            match ext.respond(id, response.clone()) {
                Ok(true) => return,
                Ok(false) => {}
                Err(e) => return self.fail(i, &e),
            }
        }
    }

    fn process_done(
        &mut self,
        run: u64,
        result: Result<kalem_core::extensions::ProcessExit, String>,
    ) {
        let result = result.map(|r| x::process::Exit {
            status: r.status,
            stdout: r.stdout,
            stderr: r.stderr,
            truncated: r.truncated,
        });
        for i in 0..self.list.len() {
            PLUGIN.with(|p| p.set(i as u64 + 1));
            let Some(ext) = self.list[i].extension.as_mut() else {
                continue;
            };
            match ext.process_done(run, result.clone()) {
                Ok(true) => return,
                Ok(false) => {}
                Err(e) => return self.fail(i, &e),
            }
        }
    }

    fn panel_event(&mut self, panel: &str, key: &str, event: &kalem_core::extensions::PanelEvent) {
        use kalem_core::extensions::PanelEvent as E;
        let event = match event {
            E::Clicked => x::PanelEvent::Clicked,
            E::Changed(t) => x::PanelEvent::Changed(t.clone()),
            E::Submitted(t) => x::PanelEvent::Submitted(t.clone()),
            E::Toggled(b) => x::PanelEvent::Toggled(*b),
            E::Expanded(b) => x::PanelEvent::Expanded(*b),
        };
        for i in 0..self.list.len() {
            PLUGIN.with(|p| p.set(i as u64 + 1));
            let Some(ext) = self.list[i].extension.as_mut() else {
                continue;
            };
            match ext.panel_event(panel, key, &event) {
                Ok(true) => return,
                Ok(false) => {}
                Err(e) => return self.fail(i, &e),
            }
        }
    }
}

/// `event` in the API's terms.
fn to_wit(event: &Event) -> x::Event {
    use x::api;
    let path = |p: &std::path::Path| p.to_string_lossy().into_owned();
    match event {
        Event::AppReady => x::Event::AppReady,
        Event::DocumentOpen { doc, path: p } => x::Event::DocumentOpen(api::DocumentOpened {
            document: doc.0,
            path: p.as_deref().map(path),
        }),
        Event::DocumentClose { doc } => x::Event::DocumentClose(doc.0),
        Event::DocumentBeforeSave { doc, path: p } => {
            x::Event::DocumentBeforeSave(api::DocumentSaved {
                document: doc.0,
                path: path(p),
            })
        }
        Event::DocumentAfterSave { doc, path: p } => {
            x::Event::DocumentAfterSave(api::DocumentSaved {
                document: doc.0,
                path: path(p),
            })
        }
        Event::DocumentChanged {
            doc,
            version,
            ranges,
        } => x::Event::DocumentChanged(api::DocumentChanged {
            document: doc.0,
            version: *version,
            ranges: ranges
                .iter()
                .map(|r| api::Range {
                    start: r.start as u64,
                    end: r.end as u64,
                })
                .collect(),
        }),
        Event::SelectionChanged { doc, anchor, head } => {
            x::Event::SelectionChanged(api::SelectionChanged {
                document: doc.0,
                anchor: *anchor as u64,
                head: *head as u64,
            })
        }
        Event::HeadlineTodoChanged {
            doc,
            headline,
            from,
            to,
        } => x::Event::HeadlineTodoChanged(api::TodoChanged {
            document: doc.0,
            headline: *headline as u64,
            from: from.clone(),
            to: to.clone(),
        }),
        Event::HeadlineTagsChanged {
            doc,
            headline,
            tags,
        } => x::Event::HeadlineTagsChanged(api::TagsChanged {
            document: doc.0,
            headline: *headline as u64,
            tags: tags.clone(),
        }),
        Event::HeadlineScheduled {
            doc,
            headline,
            timestamp,
        } => x::Event::HeadlineScheduled(api::Scheduled {
            document: doc.0,
            headline: *headline as u64,
            timestamp: timestamp.clone(),
        }),
        Event::TableBeforeRecalc { doc, table } => x::Event::TableBeforeRecalc(api::TableEvent {
            document: doc.0,
            table: *table as u64,
        }),
        Event::TableRecalculated { doc, table } => x::Event::TableRecalculated(api::TableEvent {
            document: doc.0,
            table: *table as u64,
        }),
        Event::BabelBeforeExecute {
            doc,
            block,
            language,
        } => x::Event::BabelBeforeExecute(api::BabelBefore {
            document: doc.0,
            block: *block as u64,
            language: language.clone(),
        }),
        Event::BabelAfterExecute {
            doc,
            block,
            language,
            success,
        } => x::Event::BabelAfterExecute(api::BabelAfter {
            document: doc.0,
            block: *block as u64,
            language: language.clone(),
            success: *success,
        }),
        Event::ExportBefore { doc, backend } => x::Event::ExportBefore(api::ExportBefore {
            document: doc.0,
            backend: backend.clone(),
        }),
        Event::ExportAfter {
            doc,
            backend,
            output,
        } => x::Event::ExportAfter(api::ExportAfter {
            document: doc.0,
            backend: backend.clone(),
            output: output.as_deref().map(path),
        }),
        Event::WorkspaceFileChanged { path: p } => x::Event::WorkspaceFileChanged(path(p)),
    }
}

/// The installed extension plugins: a plugin with a `main` component that
/// opens no files.
fn installed() -> Vec<Loaded> {
    let mut list = Vec::new();
    for p in kalem_core::plugin_store::installed() {
        let Ok(text) = std::fs::read_to_string(p.dir.join("plugin.json")) else {
            continue;
        };
        let Ok(m) = serde_json::from_str::<serde_json::Value>(&text) else {
            continue;
        };
        let Some(main) = m["main"].as_str() else {
            continue;
        };
        if m["opens"].as_array().is_some_and(|a| !a.is_empty()) {
            continue;
        }
        let strings = |v: &serde_json::Value| -> Vec<String> {
            v.as_array()
                .map(|a| {
                    a.iter()
                        .filter_map(|e| e.as_str().map(str::to_string))
                        .collect()
                })
                .unwrap_or_default()
        };
        let applies = kalem_core::applies::Applies::of(&m);
        let mut activation = strings(&m["activation"]);
        if activation.is_empty() {
            activation.push(
                if applies.is_empty() {
                    "onStartup"
                } else {
                    "onFile"
                }
                .into(),
            );
        }
        let defaults = Limits::default();
        let limits = Limits {
            memory: m["limits"]["memory_mb"]
                .as_u64()
                .map_or(defaults.memory, |mb| (mb as usize) << 20),
            time: m["limits"]["time_ms"]
                .as_u64()
                .map_or(defaults.time, std::time::Duration::from_millis),
        };
        let id = p.id.rsplit('.').next().unwrap_or(&p.id).to_string();
        list.push(Loaded {
            menus: manifest_menus(&id, &m),
            buttons: manifest_buttons(&id, &m),
            layers: manifest_layers(&id, &m),
            id,
            full: p.id.clone(),
            grants: x::Grants::from_permissions(&strings(&m["permissions"])),
            file: p.dir.join(main),
            activation,
            applies,
            limits,
            extension: None,
            failed: false,
        });
    }
    list
}

/// Loads the installed extension plugins on a thread, starts those that
/// start with Kalem, and offers them to the editors; their commands and
/// keys reach the editors as they register them. Without any, nothing
/// starts.
pub(crate) fn load() {
    let list = installed();
    if list.is_empty() {
        return;
    }
    let _ = std::thread::Builder::new()
        .name("kalem-extensions".into())
        .spawn(move || {
            let cache = kalem_core::logging::state_dir().map(|d| d.join("plugin-cache"));
            let host = match Host::new(cache) {
                Ok(h) => Arc::new(h),
                Err(e) => {
                    kalem_core::jobs::notice(format!("Plugins cannot run: {e}"), true);
                    return;
                }
            };
            let mut plugins = Plugins { host, list };
            for i in 0..plugins.list.len() {
                if plugins.list[i].activation.iter().any(|a| a == "onStartup") {
                    plugins.activate(i);
                }
            }
            kalem_core::extensions::install(Box::new(plugins));
        });
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_manifests_menus_hold_the_plugins_own_commands() {
        let m: serde_json::Value = serde_json::from_str(
            r#"{"menus": [
                {"title": "Git", "when": "vcs == git || textType == git-status",
                 "items": ["git.status", "-", "git.commit", "file.save", "other.x"]},
                {"title": "Broken", "when": "vcs ==", "items": ["git.status"]},
                {"title": " ", "items": ["git.status"]},
                {"title": "Always", "items": ["git.fetch"]}
            ]}"#,
        )
        .unwrap();
        let menus = super::manifest_menus("git", &m);
        assert_eq!(menus.len(), 2, "{menus:?}");
        assert_eq!(menus[0].title, "Git");
        assert_eq!(menus[0].items, ["git.status", "-", "git.commit"]);
        assert!(menus[0].when.is_some());
        assert_eq!(
            (menus[1].title.as_str(), menus[1].when.is_none()),
            ("Always", true)
        );
        assert!(super::manifest_menus("git", &serde_json::json!({})).is_empty());
    }

    #[test]
    fn a_manifests_buttons_run_the_plugins_own_commands() {
        let m: serde_json::Value = serde_json::from_str(
            r#"{"buttons": [
                {"title": "Git", "command": "git.status", "when": "vcs == git"},
                {"title": "Save", "command": "file.save"},
                {"title": "Broken", "command": "git.status", "when": "vcs =="},
                {"title": " ", "command": "git.status"},
                {"title": "No command"},
                {"title": "Fetch", "command": "git.fetch"}
            ]}"#,
        )
        .unwrap();
        let buttons = super::manifest_buttons("git", &m);
        let shown: Vec<_> = buttons
            .iter()
            .map(|b| (b.title.as_str(), b.command.as_str(), b.when.is_some()))
            .collect();
        assert_eq!(
            shown,
            [("Git", "git.status", true), ("Fetch", "git.fetch", false)]
        );
        assert!(super::manifest_buttons("git", &serde_json::json!({})).is_empty());
    }
}
