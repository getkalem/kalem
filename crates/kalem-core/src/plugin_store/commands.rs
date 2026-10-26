//! The plugins' commands (T3.7.9): the plugin list, installing, removing
//! and managing plugins and their sources, the plugin a file needs offered
//! when none opens it, and new workbooks and documents from a plugin's
//! formats or a template. Registered with the other built-in commands
//! (`crate::builtin::commands`).

use serde_json::Value;

use crate::builtin::set_setting;
use crate::command::{
    Command, CommandError, CommandResult, EditorContext, Request, arg_bool, cmd, request,
};

/// Plugins installed, updated and removed from inside Kalem
/// (`plugin_store`, T3.3.3). Downloads run as background jobs; what was
/// found is offered as a list to confirm, with its permissions.
pub(crate) fn plugin_commands() -> Vec<Command> {
    use crate::palette::{PaletteItem, invocation};
    use serde_json::json;
    /// An item of the plugins' lists: what it does, and what is said of
    /// it after (a version, what an install brings).
    fn item(id: String, title: String, detail: String) -> PaletteItem {
        PaletteItem {
            also: title.clone(),
            id,
            title,
            category: String::new(),
            detail,
            keys: String::new(),
        }
    }
    fn index_urls(ctx: &EditorContext<'_>) -> Vec<String> {
        crate::plugin_store::index_urls(ctx.config)
    }
    /// The user's plugin indexes (`plugins.sources`).
    fn sources(ctx: &EditorContext<'_>) -> Vec<String> {
        ctx.config
            .strings("plugins.sources")
            .into_iter()
            .map(str::to_string)
            .collect()
    }
    /// Downloads `source` in the background, then offers to install it.
    /// Fetches a plugin and offers to install it; `open`, a file to open
    /// once it is installed (the file it was installed for, T3.7.9).
    fn start_install(
        ctx: &mut EditorContext<'_>,
        source: String,
        open: Option<String>,
    ) -> CommandResult {
        let index = index_urls(ctx);
        crate::jobs::spawn(
            crate::tr!("plugin-fetching", source = source.as_str()),
            move || match crate::plugin_store::prepare(&source, &index) {
                Ok(p) => {
                    let lines = crate::plugin_store::summary(&p);
                    let staging = p.staging.to_string_lossy().into_owned();
                    let verb_key = if p.replaces.is_some() {
                        "plugin-verb-update"
                    } else {
                        "plugin-verb-install"
                    };
                    let mut confirm = json!({
                        "staging": staging, "source": p.source, "manifest": p.manifest
                    });
                    if let Some(open) = &open {
                        confirm["open"] = json!(open);
                    }
                    let mut items = vec![item(
                        invocation("plugin.confirmInstall", &confirm),
                        crate::tr!(verb_key, what = lines[0].as_str()),
                        lines[1..].join(" · "),
                    )];
                    items.push(item(
                        invocation("plugin.cancelInstall", &json!({ "staging": staging })),
                        crate::tr!("plugin-cancel"),
                        String::new(),
                    ));
                    crate::jobs::offer(items);
                    crate::jobs::Finished {
                        message: crate::tr!(
                            "plugin-ready",
                            name = p.name.as_str(),
                            version = p.version.as_str()
                        ),
                        error: false,
                        open: None,
                    }
                }
                Err(e) => crate::jobs::Finished {
                    message: e,
                    error: true,
                    open: None,
                },
            },
        );
        Ok(())
    }
    vec![
        // Where plugins are listed: the official index and the user's own
        // (a fork of it on GitHub, or an index of their own plugins).
        cmd(
            "plugin.sources",
            "Plugin Sources…",
            "Plugins",
            &[],
            None,
            |ctx, _| {
                let mut items = vec![item(
                    invocation("plugin.addSource", &json!({})),
                    crate::tr!("plugin-source-add"),
                    String::new(),
                )];
                for url in sources(ctx) {
                    items.push(item(
                        invocation("plugin.removeSource", &json!({ "url": url })),
                        crate::tr!("plugin-source-remove", url = url.as_str()),
                        crate::tr!("plugin-source-yours"),
                    ));
                }
                let main = match ctx.config.str("plugins.index") {
                    "" => crate::plugin_store::DEFAULT_INDEX.to_string(),
                    s => s.to_string(),
                };
                items.push(item(
                    invocation("plugin.setIndex", &json!({})),
                    crate::tr!("plugin-source-main", url = main.as_str()),
                    crate::tr!("plugin-source-official"),
                ));
                request(ctx, Request::Choose(items))
            },
        ),
        cmd(
            "plugin.addSource",
            "Add Plugin Source…",
            "Plugins",
            &[],
            None,
            |ctx, args| {
                let Some(url) = args["url"]
                    .as_str()
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                else {
                    return request(
                        ctx,
                        Request::Ask {
                            command: "plugin.addSource".into(),
                            args: json!({}),
                            arg: "url".into(),
                        },
                    );
                };
                let mut list = sources(ctx);
                if !list.iter().any(|u| u == url) {
                    list.insert(0, url.to_string());
                }
                ctx.messages
                    .push(crate::tr!("plugin-source-added", url = url));
                set_setting(ctx, "plugins.sources", json!(list))
            },
        ),
        cmd(
            "plugin.removeSource",
            "Remove Plugin Source",
            "Plugins",
            &[],
            None,
            |ctx, args| {
                let url = args["url"].as_str().unwrap_or_default().to_string();
                let list: Vec<String> = sources(ctx).into_iter().filter(|u| *u != url).collect();
                ctx.messages
                    .push(crate::tr!("plugin-source-removed", url = url.as_str()));
                set_setting(ctx, "plugins.sources", json!(list))
            },
        ),
        cmd(
            "plugin.setIndex",
            "Set Official Plugin Index…",
            "Plugins",
            &[],
            None,
            |ctx, args| {
                let Some(url) = args["url"].as_str().map(str::trim) else {
                    return request(
                        ctx,
                        Request::Ask {
                            command: "plugin.setIndex".into(),
                            args: json!({}),
                            arg: "url".into(),
                        },
                    );
                };
                // Empty: the default again.
                let url = if url.is_empty() {
                    crate::plugin_store::DEFAULT_INDEX
                } else {
                    url
                };
                set_setting(ctx, "plugins.index", json!(url))
            },
        ),
        cmd(
            "plugin.browse",
            "Browse Plugins",
            "Plugins",
            &[],
            None,
            |ctx, _| {
                let index = index_urls(ctx);
                crate::jobs::spawn(crate::tr!("plugin-reading-index"), move || {
                    match crate::plugin_store::fetch_indexes(&index) {
                        Ok(entries) => {
                            // Remembers the versions for Installed Plugins.
                            let _ = crate::plugin_store::updates(&entries);
                            let installed = crate::plugin_store::installed();
                            let items: Vec<PaletteItem> = entries
                                .iter()
                                .map(|e| {
                                    let state = match installed.iter().find(|i| i.id == e.id) {
                                        Some(i) if i.version == e.version => {
                                            crate::tr!(
                                                "plugin-installed-version",
                                                version = i.version.as_str()
                                            )
                                        }
                                        Some(i) => crate::tr!(
                                            "plugin-installed-available",
                                            version = i.version.as_str(),
                                            available = e.version.as_str()
                                        ),
                                        // A component not released: built from
                                        // its source only.
                                        None if !e.declarative && e.download.is_none() => {
                                            crate::tr!("plugin-no-release")
                                        }
                                        None => e.version.clone(),
                                    };
                                    item(
                                        invocation("plugin.install", &json!({ "source": e.id })),
                                        format!("{} — {}", e.name, e.description),
                                        state,
                                    )
                                })
                                .collect();
                            let n = items.len();
                            // A plugin of one's own, from its repository.
                            let mut items = items;
                            items.insert(
                                0,
                                item(
                                    invocation("plugin.installGitHub", &json!({})),
                                    crate::tr!("plugin-github-item"),
                                    crate::tr!("plugin-github-item-detail"),
                                ),
                            );
                            crate::jobs::offer(items);
                            crate::jobs::Finished {
                                message: crate::tr!("plugin-index-count", count = n),
                                error: false,
                                open: None,
                            }
                        }
                        Err(e) => crate::jobs::Finished {
                            message: e,
                            error: true,
                            open: None,
                        },
                    }
                });
                Ok(())
            },
        ),
        cmd(
            "plugin.install",
            "Install Plugin…",
            "Plugins",
            &[],
            None,
            |ctx, args| match args["source"]
                .as_str()
                .map(str::trim)
                .filter(|s| !s.is_empty())
            {
                Some(s) => {
                    start_install(ctx, s.to_string(), args["open"].as_str().map(str::to_owned))
                }
                None => request(
                    ctx,
                    Request::Ask {
                        command: "plugin.install".into(),
                        args: json!({}),
                        arg: "source".into(),
                    },
                ),
            },
        ),
        // The plugin the status bar suggested when a file opened (a
        // language's, a Logseq graph's), installed after the usual
        // confirmation.
        cmd(
            "plugin.installSuggested",
            "Install Suggested Plugin",
            "Plugins",
            &[],
            None,
            |ctx, _| match crate::plugin_store::suggested() {
                Some(id) => start_install(ctx, id, None),
                None => Err(CommandError::new(crate::tr!("plugin-none-suggested"))),
            },
        ),
        // A file Kalem cannot open yet (not text, no viewer installed): the
        // released plugins of the indexes that open it offered, installed
        // and the file opened at a choice; else the system's application
        // (T3.7.9). No plugin is built into Kalem: each is installed when
        // it is wanted.
        cmd(
            "plugin.forFile",
            "Find a Plugin for the File",
            "Plugins",
            &[],
            None,
            |ctx, args| {
                let Some(path) = args["path"].as_str().map(str::to_owned) else {
                    return Err(CommandError::new(crate::tr!("msg-no-document")));
                };
                if args["how"].as_str() == Some("system") {
                    return request(
                        ctx,
                        Request::OpenLink(crate::input::LinkAction::System(
                            std::path::PathBuf::from(&path),
                        )),
                    );
                }
                offer_plugins(ctx, Some(path.clone()), &path);
                Ok(())
            },
        ),
        // A file protected by a password (a PDF): asked for when it would
        // not open, kept for the session, the file opened again.
        cmd(
            "file.openWithPassword",
            "Open with Password…",
            "File",
            &[],
            None,
            |ctx, args| {
                let Some(path) = args["path"].as_str().map(str::to_owned) else {
                    return Err(CommandError::new(crate::tr!("msg-no-document")));
                };
                let Some(password) = args["password"].as_str() else {
                    return request(
                        ctx,
                        Request::Ask {
                            command: "file.openWithPassword".into(),
                            args: json!({ "path": path }),
                            arg: "password".into(),
                        },
                    );
                };
                let path = std::path::PathBuf::from(&path);
                let path = dunce::canonicalize(&path).unwrap_or(path);
                crate::viewer::remember_password(&path, password);
                request(
                    ctx,
                    Request::Open {
                        path: Some(path.display().to_string()),
                    },
                )
            },
        ),
        // A plugin of one's own (or anyone's) from its GitHub repository:
        // a declarative one as its folder is, a component one with its
        // build from the repository's releases.
        cmd(
            "plugin.installGitHub",
            "Install Plugin from GitHub…",
            "Plugins",
            &[],
            None,
            |ctx, args| match args["link"]
                .as_str()
                .map(str::trim)
                .filter(|s| !s.is_empty())
            {
                Some(link) => match crate::plugin_store::github_link(link) {
                    Some(source) => start_install(ctx, source, None),
                    None => Err(CommandError::new(crate::tr!(
                        "plugin-not-github",
                        link = link
                    ))),
                },
                None => request(
                    ctx,
                    Request::Ask {
                        command: "plugin.installGitHub".into(),
                        args: json!({}),
                        arg: "link".into(),
                    },
                ),
            },
        ),
        cmd(
            "plugin.confirmInstall",
            "Confirm Plugin Installation",
            "Plugins",
            &[],
            None,
            |ctx, args| {
                let staging =
                    std::path::PathBuf::from(args["staging"].as_str().unwrap_or_default());
                let source = args["source"].as_str().unwrap_or_default();
                // Read again from the staging folder: what is installed is
                // what the user was shown, its manifest (what it asks for,
                // which plugins.toml records as approved) unchanged since.
                let p = crate::plugin_store::prepared_at(&staging, source)
                    .map_err(CommandError::new)?;
                if args["manifest"].as_str() != Some(p.manifest.as_str()) {
                    crate::plugin_store::discard(&staging);
                    return Err(CommandError::new(crate::tr!(
                        "plugin-changed-since-shown",
                        name = p.name.as_str()
                    )));
                }
                let dir = crate::plugin_store::install(&p).map_err(CommandError::new)?;
                ctx.messages.push(crate::tr!(
                    "plugin-installed",
                    name = p.name.as_str(),
                    version = p.version.as_str(),
                    dir = dir.display().to_string()
                ));
                // Installed for a file: opened once the plugin opens it. The
                // editor's watch of the plugins chooses the viewers again
                // within a second or two of an install.
                if let Some(open) = args["open"].as_str().map(std::path::PathBuf::from) {
                    let name = p.name.clone();
                    crate::jobs::spawn(
                        crate::tr!("plugin-starting", name = name.as_str()),
                        move || {
                            let deadline =
                                std::time::Instant::now() + std::time::Duration::from_secs(15);
                            while crate::viewer::for_file(&open).is_none()
                                && std::time::Instant::now() < deadline
                            {
                                std::thread::sleep(std::time::Duration::from_millis(200));
                            }
                            if crate::viewer::for_file(&open).is_some() {
                                crate::jobs::Finished {
                                    message: String::new(),
                                    error: false,
                                    open: Some(crate::input::LinkAction::File {
                                        path: open.display().to_string(),
                                        search: None,
                                    }),
                                }
                            } else {
                                crate::jobs::Finished {
                                    message: crate::tr!(
                                        "plugin-did-not-start",
                                        name = name.as_str()
                                    ),
                                    error: true,
                                    open: None,
                                }
                            }
                        },
                    );
                }
                Ok(())
            },
        ),
        cmd(
            "plugin.cancelInstall",
            "Cancel Plugin Installation",
            "Plugins",
            &[],
            None,
            |ctx, args| {
                crate::plugin_store::discard(std::path::Path::new(
                    args["staging"].as_str().unwrap_or_default(),
                ));
                ctx.messages.push(crate::tr!("plugin-not-installed-cancel"));
                Ok(())
            },
        ),
        cmd(
            "plugin.list",
            "Installed Plugins",
            "Plugins",
            &[],
            None,
            |ctx, _| {
                let items: Vec<PaletteItem> = crate::plugin_store::installed()
                    .into_iter()
                    .map(|p| {
                        let from = p
                            .source
                            .clone()
                            .unwrap_or_else(|| p.dir.display().to_string());
                        let detail = match crate::plugin_store::available(&p.id, &p.version) {
                            Some(v) => {
                                crate::tr!("plugin-available-from", version = v, from = from)
                            }
                            None => from,
                        };
                        item(
                            invocation("plugin.manage", &json!({ "id": p.id })),
                            format!("{} {}", p.name, p.version),
                            detail,
                        )
                    })
                    .collect();
                if items.is_empty() {
                    ctx.messages.push(crate::tr!("plugin-none-installed"));
                    return Ok(());
                }
                request(ctx, Request::Choose(items))
            },
        ),
        cmd(
            "plugin.manage",
            "Manage Plugin",
            "Plugins",
            &[],
            None,
            |ctx, args| {
                let id = args["id"].as_str().unwrap_or_default();
                let p = crate::plugin_store::installed()
                    .into_iter()
                    .find(|p| p.id == id)
                    .ok_or_else(|| {
                        CommandError::new(crate::tr!("plugin-not-installed", id = id))
                    })?;
                let mut items = Vec::new();
                if let Some(src) = &p.source {
                    items.push(item(
                        invocation("plugin.install", &json!({ "source": src })),
                        crate::tr!("plugin-update", name = p.name.as_str()),
                        crate::tr!("plugin-from-short", source = src.as_str()),
                    ));
                }
                items.push(item(
                    invocation("plugin.remove", &json!({ "id": p.id })),
                    crate::tr!("plugin-remove", name = p.name.as_str()),
                    crate::tr!("plugin-asks-first"),
                ));
                items.push(item(
                    invocation("file.open", &json!({ "path": p.dir })),
                    crate::tr!("plugin-show-folder"),
                    p.dir.display().to_string(),
                ));
                request(ctx, Request::Choose(items))
            },
        ),
        cmd(
            "plugin.remove",
            "Remove Plugin",
            "Plugins",
            &[],
            None,
            |ctx, args| {
                let id = args["id"].as_str().unwrap_or_default();
                let p = crate::plugin_store::installed()
                    .into_iter()
                    .find(|p| p.id == id)
                    .ok_or_else(|| {
                        CommandError::new(crate::tr!("plugin-not-installed", id = id))
                    })?;
                request(
                    ctx,
                    Request::Choose(vec![
                        item(
                            invocation("plugin.removeConfirmed", &json!({ "id": p.id })),
                            crate::tr!(
                                "plugin-remove-version",
                                name = p.name.as_str(),
                                version = p.version.as_str()
                            ),
                            crate::tr!("plugin-deletes", dir = p.dir.display().to_string()),
                        ),
                        item(
                            "plugin.list".into(),
                            crate::tr!("plugin-cancel"),
                            String::new(),
                        ),
                    ]),
                )
            },
        ),
        cmd(
            "plugin.removeConfirmed",
            "Remove Plugin Now",
            "Plugins",
            &[],
            None,
            |ctx, args| {
                let name = crate::plugin_store::remove(args["id"].as_str().unwrap_or_default())
                    .map_err(CommandError::new)?;
                ctx.messages.push(crate::tr!("plugin-removed", name = name));
                Ok(())
            },
        ),
    ]
}

/// Where a new workbook goes: the path asked (the folder of the file
/// open, `Book1.xlsx` or the next free name offered), its extension added,
/// and a file there replaced only when chosen. `Ok(None)` when it asked.
fn new_file_target(
    ctx: &mut EditorContext<'_>,
    id: &str,
    args: &Value,
    stem: &str,
    ext: &str,
) -> Result<Option<std::path::PathBuf>, CommandError> {
    let dir = ctx
        .document
        .as_deref()
        .and_then(|d| d.meta.path.as_ref())
        .and_then(|p| p.parent().map(|d| d.to_path_buf()))
        .or_else(|| std::env::var_os("HOME").map(std::path::PathBuf::from))
        .unwrap_or_default();
    let Some(path) = args.get("path").and_then(Value::as_str) else {
        let free = (1..)
            .map(|n| dir.join(format!("{stem}{n}.{ext}")))
            .find(|p| !p.exists())
            .unwrap_or_else(|| dir.join(format!("{stem}.{ext}")));
        let mut a = args.clone();
        a["path_default"] = Value::String(free.display().to_string());
        request(
            ctx,
            Request::Ask {
                command: id.into(),
                args: a,
                arg: "path".into(),
            },
        )?;
        return Ok(None);
    };
    let mut target = std::path::PathBuf::from(crate::settings::expand_home(path.trim()));
    if target.is_relative() {
        target = dir.join(target);
    }
    if target.extension().is_none() {
        target.set_extension(ext);
    }
    if target.exists() && !arg_bool(args, "replace") {
        let name = target
            .file_name()
            .map_or(String::new(), |f| f.to_string_lossy().into_owned());
        let mut a = args.clone();
        a["path"] = Value::String(target.display().to_string());
        a["replace"] = Value::Bool(true);
        let item = crate::palette::PaletteItem {
            id: crate::palette::invocation(id, &a),
            title: format!("Replace {name}"),
            category: format!("{name} exists"),
            detail: String::new(),
            keys: String::new(),
            also: String::new(),
        };
        request(ctx, Request::Choose(vec![item]))?;
        return Ok(None);
    }
    Ok(Some(target))
}

/// The released plugins of the indexes that open a file named `name` (by
/// what each declares: its extension, its first bytes) offered to install, read in the background, with the
/// system's application when `open` is a file: for a file Kalem cannot
/// open yet (`plugin.forFile`), the file then opened; or for a command
/// that needs such a plugin (New Workbook), `open` none. No plugin is
/// built into Kalem: each is installed when it is wanted (T3.7.9).
pub(crate) fn offer_plugins(ctx: &mut EditorContext<'_>, open: Option<String>, name: &str) {
    let item = |id: String, title: String, detail: String| crate::palette::PaletteItem {
        also: title.clone(),
        id,
        title,
        category: String::new(),
        detail,
        keys: String::new(),
    };
    let file = std::path::Path::new(name)
        .file_name()
        .map_or(name.to_string(), |n| n.to_string_lossy().into_owned());
    let kind = std::path::Path::new(name)
        .extension()
        .map_or(String::new(), |e| format!(".{}", e.to_string_lossy()));
    let name = name.to_string();
    let index = crate::plugin_store::index_urls(ctx.config);
    crate::jobs::spawn(
        crate::tr!("plugin-finding", file = file.as_str()),
        move || {
            let system = open.as_ref().map(|path| {
                item(
                    crate::palette::invocation(
                        "plugin.forFile",
                        &serde_json::json!({ "path": path, "how": "system" }),
                    ),
                    crate::tr!("plugin-open-system"),
                    String::new(),
                )
            });
            // The index as last read (at a start, or by Browse Plugins)
            // when it cannot be read now.
            let read = match crate::plugin_store::fetch_indexes(&index) {
                Err(e) => {
                    let known = crate::plugin_store::known_index();
                    if known.is_empty() { Err(e) } else { Ok(known) }
                }
                ok => ok,
            };
            match read {
                Ok(entries) => {
                    // Its first bytes, for the plugins that know a file
                    // by them; a kind of file named alone has none.
                    let head = open
                        .as_ref()
                        .and_then(|p| crate::applies::head_of(std::path::Path::new(p)));
                    let found = crate::plugin_store::opening(
                        &entries,
                        std::path::Path::new(&name),
                        head.as_deref(),
                    );
                    let mut items: Vec<crate::palette::PaletteItem> = found
                        .iter()
                        .map(|e| {
                            let (args, title) = match &open {
                                Some(path) => (
                                    serde_json::json!({ "source": e.id, "open": path }),
                                    crate::tr!(
                                        "plugin-install-to-open",
                                        name = e.name.as_str(),
                                        version = e.version.as_str(),
                                        file = file.as_str()
                                    ),
                                ),
                                None => (
                                    serde_json::json!({ "source": e.id }),
                                    crate::tr!(
                                        "plugin-install-for-kind",
                                        name = e.name.as_str(),
                                        version = e.version.as_str(),
                                        kind = kind.as_str()
                                    ),
                                ),
                            };
                            item(
                                crate::palette::invocation("plugin.install", &args),
                                title,
                                e.description.clone(),
                            )
                        })
                        .collect();
                    let message = match (items.is_empty(), &open) {
                        (true, Some(_)) => crate::tr!("plugin-none-opens", file = file.as_str()),
                        (true, None) => crate::tr!("plugin-none-for-kind", kind = kind.as_str()),
                        (false, Some(_)) => {
                            crate::tr!("plugin-not-installed-for", file = file.as_str())
                        }
                        (false, None) => {
                            crate::tr!("plugin-needed-for-kind", kind = kind.as_str())
                        }
                    };
                    items.extend(system);
                    if !items.is_empty() {
                        crate::jobs::offer(items);
                    }
                    crate::jobs::Finished {
                        message,
                        error: false,
                        open: None,
                    }
                }
                Err(e) => {
                    if let Some(system) = system {
                        crate::jobs::offer(vec![system]);
                    }
                    crate::jobs::Finished {
                        message: crate::tr!(
                            "plugin-index-unread-for",
                            file = file.as_str(),
                            error = e
                        ),
                        error: true,
                        open: None,
                    }
                }
            }
        },
    );
}

/// The workbook plugin, which makes workbooks; when it is not installed,
/// the plugins that do offered to install, and none.
fn workbook_or_offer(
    ctx: &mut EditorContext<'_>,
) -> Option<std::sync::Arc<dyn kalem_viewer::Viewer>> {
    match crate::workbook_io::workbook_viewer() {
        Ok(v) => Some(v),
        Err(_) => {
            offer_plugins(ctx, None, "Book.xlsx");
            None
        }
    }
}

/// New Workbook: a blank workbook of one sheet, written where asked and
/// opened.
pub(crate) fn new_workbook(ctx: &mut EditorContext<'_>, args: &Value) -> CommandResult {
    let Some(viewer) = workbook_or_offer(ctx) else {
        return Ok(());
    };
    let Some(target) = new_file_target(ctx, "app.newWorkbook", args, "Book", "xlsx")? else {
        return Ok(());
    };
    let sheet = kalem_viewer::NewSheet {
        name: "Sheet1".into(),
        rows: Vec::new(),
    };
    let bytes = viewer
        .new_file("xlsx", &[sheet])
        .map_err(|e| CommandError::new(e.to_string()))?;
    std::fs::write(&target, bytes).map_err(|e| CommandError::new(e.to_string()))?;
    request(
        ctx,
        Request::Open {
            path: Some(target.display().to_string()),
        },
    )
}

/// New from Template: a template (`.xltx`, `.xltm`) chosen, a workbook made
/// of it written where asked and opened.
pub(crate) fn new_from_template(ctx: &mut EditorContext<'_>, args: &Value) -> CommandResult {
    const ID: &str = "app.newFromTemplate";
    let Some(template) = args
        .get("template")
        .and_then(Value::as_str)
        .map(str::to_owned)
    else {
        return request(
            ctx,
            Request::PickFile {
                command: ID.into(),
                arg: "template".into(),
                args: args.clone(),
            },
        );
    };
    let template = std::path::PathBuf::from(crate::settings::expand_home(&template));
    let macros = template
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("xltm"));
    let stem = template
        .file_stem()
        .map_or("Book".into(), |s| s.to_string_lossy().into_owned());
    let ext = if macros { "xlsm" } else { "xlsx" };
    let Some(viewer) = workbook_or_offer(ctx) else {
        return Ok(());
    };
    let Some(target) = new_file_target(ctx, ID, args, &stem, ext)? else {
        return Ok(());
    };
    let bytes = std::fs::read(&template).map_err(|e| CommandError::new(e.to_string()))?;
    // The template opened by the workbook plugin and written as a workbook.
    let bytes = crate::workbook_io::open_bytes(viewer.as_ref(), &template, bytes)
        .and_then(|mut d| d.save_as(ext).map(|o| o.bytes).map_err(|e| e.to_string()))
        .map_err(CommandError::new)?;
    std::fs::write(&target, bytes).map_err(|e| CommandError::new(e.to_string()))?;
    request(
        ctx,
        Request::Open {
            path: Some(target.display().to_string()),
        },
    )
}

/// Open as Workbook (Excel's Text Import Wizard): the file's records
/// read with a delimiter and an encoding chosen, each column as General,
/// Text, a date in an order, or left out; written as a workbook beside it
/// (or where asked) and opened.
pub(crate) fn open_as_workbook(ctx: &mut EditorContext<'_>, args: &Value) -> CommandResult {
    const ID: &str = "csv.openAsWorkbook";
    // The workbook plugin makes the workbook: offered first when missing.
    if workbook_or_offer(ctx).is_none() {
        return Ok(());
    }
    let item = |a: Value, title: &str, category: &str| crate::palette::PaletteItem {
        id: crate::palette::invocation(ID, &a),
        title: title.into(),
        category: category.into(),
        detail: String::new(),
        keys: String::new(),
        also: title.into(),
    };
    let with = |key: &str, v: &str| {
        let mut a = args.clone();
        a[key] = Value::String(v.into());
        a
    };
    let d = ctx.doc()?;
    let detected = crate::csv::detect(d.text().as_str()).delimiter;
    let path = d.meta.path.clone();
    let Some(delimiter) = args
        .get("delimiter")
        .and_then(Value::as_str)
        .map(str::to_owned)
    else {
        let mut choices = vec![
            (",", "Comma"),
            (";", "Semicolon"),
            ("tab", "Tab"),
            ("|", "Vertical Bar"),
            (" ", "Space"),
        ];
        // The delimiter the file seems to have, first.
        let found = if detected == b'\t' {
            "tab".to_string()
        } else {
            (detected as char).to_string()
        };
        if let Some(k) = choices.iter().position(|c| c.0 == found) {
            let c = choices.remove(k);
            choices.insert(0, c);
        }
        let items = choices
            .iter()
            .enumerate()
            .map(|(k, (v, t))| {
                let t = if k == 0 && *v == found {
                    format!("{t} (found)")
                } else {
                    (*t).to_string()
                };
                item(with("delimiter", v), &t, "Open as Workbook: delimiter")
            })
            .collect();
        return request(ctx, Request::Choose(items));
    };
    let Some(encoding) = args
        .get("encoding")
        .and_then(Value::as_str)
        .map(str::to_owned)
    else {
        let items = [
            ("utf-8", "UTF-8"),
            ("windows-1254", "Turkish (Windows-1254)"),
            ("iso-8859-9", "Turkish (ISO-8859-9)"),
            ("windows-1252", "Western (Windows-1252)"),
            ("utf-16le", "UTF-16 LE"),
            ("utf-16be", "UTF-16 BE"),
        ]
        .iter()
        .map(|(v, t)| item(with("encoding", v), t, "Open as Workbook: encoding"))
        .collect();
        return request(ctx, Request::Choose(items));
    };
    let enc = encoding_rs::Encoding::for_label(encoding.as_bytes())
        .ok_or_else(|| CommandError::new(format!("{encoding} is not an encoding")))?;
    let delimiter_byte = if delimiter == "tab" {
        b'\t'
    } else {
        delimiter.as_bytes().first().copied().unwrap_or(b',')
    };
    let dialect = crate::csv::Dialect {
        delimiter: delimiter_byte,
        quote: b'"',
        header: false,
        crlf: false,
    };
    // The text as the editor holds it when it has unsaved edits; else the
    // file's bytes read again in the encoding chosen (a new file's text
    // as it is).
    let text = {
        let d = ctx.doc()?;
        match path
            .as_ref()
            .filter(|_| !d.is_modified())
            .map(std::fs::read)
        {
            Some(Ok(bytes)) => enc.decode(&bytes).0.into_owned(),
            _ => d.text().as_str().to_owned(),
        }
    };
    let rows = crate::csv::rows(&text, &dialect);
    // How numbers are written: `1,234.56` or `1.234,56` (publish_todo
    // 3.7: the second was read as text).
    let Some(decimal) = args
        .get("decimal")
        .and_then(Value::as_str)
        .map(str::to_owned)
    else {
        let comma = crate::workbook_io::guess_decimal_comma(&rows, delimiter_byte);
        let mut choices = [(".", "Point: 1,234.56"), (",", "Comma: 1.234,56")];
        if comma {
            choices.swap(0, 1);
        }
        let items = choices
            .iter()
            .enumerate()
            .map(|(k, (v, t))| {
                let t = if k == 0 {
                    format!("{t} (found)")
                } else {
                    (*t).to_string()
                };
                item(with("decimal", v), &t, "Open as Workbook: decimals")
            })
            .collect();
        return request(ctx, Request::Choose(items));
    };
    let Some(types) = args
        .get("column types")
        .and_then(Value::as_str)
        .map(str::to_owned)
    else {
        return request(
            ctx,
            Request::Ask {
                command: ID.into(),
                args: args.clone(),
                arg: "column types".into(),
            },
        );
    };
    let types = crate::workbook_io::parse_types(&types).map_err(CommandError::new)?;
    let Some(target) = args.get("path").and_then(Value::as_str).map(str::to_owned) else {
        let default = path.as_ref().map_or("Book.xlsx".into(), |p| {
            p.with_extension("xlsx").display().to_string()
        });
        let mut a = args.clone();
        a["path_default"] = Value::String(default);
        return request(
            ctx,
            Request::Ask {
                command: ID.into(),
                args: a,
                arg: "path".into(),
            },
        );
    };
    let mut target = std::path::PathBuf::from(crate::settings::expand_home(target.trim()));
    if target.is_relative()
        && let Some(dir) = path.as_ref().and_then(|p| p.parent())
    {
        target = dir.join(target);
    }
    // A workbook: `out.csv` would get a workbook's bytes otherwise.
    if !target
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("xlsx"))
    {
        target.set_extension("xlsx");
    }
    if target.exists() && !arg_bool(args, "replace") {
        let name = target
            .file_name()
            .map_or(String::new(), |f| f.to_string_lossy().into_owned());
        let mut a = args.clone();
        a["path"] = Value::String(target.display().to_string());
        a["replace"] = Value::Bool(true);
        return request(
            ctx,
            Request::Choose(vec![item(
                a,
                &format!("Replace {name}"),
                &format!("{name} exists"),
            )]),
        );
    }
    let rows = crate::workbook_io::import_rows(&rows, &types, decimal == ",");
    let sheet: String = target
        .file_stem()
        .map_or("Sheet1".into(), |s| s.to_string_lossy().into_owned())
        .chars()
        .filter(|c| !"[]:*?/\\".contains(*c))
        .take(31)
        .collect();
    let sheet = kalem_viewer::NewSheet {
        name: if sheet.is_empty() {
            "Sheet1".into()
        } else {
            sheet
        },
        rows,
    };
    let bytes = crate::workbook_io::workbook_viewer()
        .and_then(|v| v.new_file("xlsx", &[sheet]).map_err(|e| e.to_string()))
        .map_err(CommandError::new)?;
    std::fs::write(&target, bytes).map_err(|e| CommandError::new(e.to_string()))?;
    request(
        ctx,
        Request::Open {
            path: Some(target.display().to_string()),
        },
    )
}
