//! Kalem's graphical frontend (design §7.1 to §7.5), on gpui: the same
//! commands, keymaps, settings and view model as the terminal frontend,
//! drawn with Kalem's own inline layout.

// A crash ends the user's work: no `unwrap`, `expect` or `panic!`
// outside tests but where an `#[expect]` says why it cannot happen
// (roadmap R2.2).
#![warn(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

pub mod chart;
pub mod clipboard;
pub mod datepicker;
pub mod editor;
pub mod icon;
pub mod keys;
pub mod line;
pub mod math;
pub mod outline;
pub mod panels;
pub mod pictures;
pub mod plugin_panel;
pub mod preferences;
pub mod preview;
pub mod theme;
pub mod viewer;
pub mod vim;
pub mod workspace;

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

use kalem_core::CommandRegistry;
use kalem_core::keymap::{self, Keymap};
use kalem_core::settings::{self, Config};

/// The shared state: settings, commands and keys, with the user's
/// `settings.toml` and `keymap.json` in Kalem's configuration folder.
pub fn shared(config: Config) -> editor::Shared {
    shared_in(
        config,
        settings::config_dir().map(|d| d.join("settings.toml")),
    )
}

/// The shared state with the user's settings file at `settings_path` and
/// the keymap beside it: tests give a folder of their own (or none), so
/// they never read or write the user's files.
pub fn shared_in(config: Config, settings_path: Option<PathBuf>) -> editor::Shared {
    // What plugins read (`kalem.settings`); those watching a key that
    // changed are told.
    kalem_core::extensions::set_config(&config);
    let registry = CommandRegistry::with_builtins();
    let user = settings_path
        .as_deref()
        .and_then(std::path::Path::parent)
        .map(|d| d.join("keymap.json"));
    // The project list beside the settings file: a test's folder (or none,
    // the list in memory) never reaches the user's projects.
    let projects_file = settings_path
        .as_deref()
        .and_then(std::path::Path::parent)
        .map(|d| d.join("projects.toml"));
    let (entries, mut issues) = match user.as_deref().map(std::fs::read_to_string) {
        Some(Ok(text)) => {
            keymap::parse_keymap_with(&text, keymap::Origin::User, &config.vim_leader())
        }
        _ => (Vec::new(), Vec::new()),
    };
    let profile = config.keymap_profile();
    let (keymap, more) = Keymap::build_with(&registry, profile, &entries, &config.vim_leader());
    issues.extend(more);
    editor::Shared {
        // Both profiles have the Word-like keys, with Command on macOS.
        swap_primary: cfg!(target_os = "macos"),
        html_clipboard: clipboard::html,
        settings_path,
        math: math::Formulas::default(),
        pictures: Default::default(),
        projects: RefCell::new(kalem_core::projects::ProjectState::load(projects_file)),
        jobs: Rc::default(),
        completers: kalem_core::completers::Registry::with_builtins(),
        bus: Rc::new(RefCell::new({
            let mut bus = kalem_core::events::EventBus::new();
            // The plugins hear every event (`kalem_core::extensions`).
            bus.subscribe(None, kalem_core::extensions::event);
            bus
        })),
        last: Rc::default(),
        problems: std::cell::Cell::new(0),
        config,
        registry,
        keymap,
        issues,
    }
}

/// Opens the graphical editor with `path` (or an empty document).
pub fn run(path: Option<PathBuf>) {
    let user = settings::config_dir().map(|d| d.join("settings.toml"));
    let dir = path
        .as_ref()
        .and_then(|p| std::path::absolute(p).ok())
        .and_then(|p| p.parent().map(std::path::Path::to_path_buf));
    let workspace = dir.as_deref().and_then(settings::find_workspace_settings);
    let config = Config::load(user.as_deref(), workspace.as_deref());
    if let Some(p) = &path {
        editor::prefetch(p, config.parse_base());
    }
    let _ = kalem_core::logging::init(&kalem_core::logging::LogOptions::standard(&config, false));
    config.apply_process_settings();
    // Newer versions of the installed plugins, once a day.
    kalem_core::plugin_store::check_updates(&config);
    // The plugins' registrations counted before the keymap is built: a
    // plugin that starts meanwhile (from the compiled components' cache,
    // at once) is seen at the first tick. Counted after, its keys were
    // never bound.
    let plugins_at_start = kalem_core::extensions::generation();
    let shared = shared(config);
    shared.problems.set(settings::report_problems(
        &shared.config,
        &shared.issues,
        false,
    ));
    let shared = Rc::new(shared);
    // Files the system opens with Kalem (Finder, `open -a Kalem`) arrive as
    // URLs, outside the application's context: queued, opened by a task.
    let opened: Rc<RefCell<Vec<PathBuf>>> = Rc::default();
    let app = gpui_platform::application();
    let queue = opened.clone();
    app.on_open_urls(move |urls| {
        queue
            .borrow_mut()
            .extend(urls.iter().filter_map(|u| file_url_path(u)));
    });
    app.run(move |cx| {
        // The logo in the Dock rather than "exec" (`cargo run`).
        icon::set_app_icon();
        cx.bind_keys(workspace::menu_bindings(&shared));
        cx.set_menus(workspace::menus());
        // The session `SPC q l` restores.
        cx.on_app_quit(|cx| {
            workspace::save_last_session(cx);
            async {}
        })
        .detach();
        let s = shared.clone();
        cx.on_action(move |_: &workspace::OpenFile, cx| workspace::open_file(s.clone(), cx));
        // Started from an app bundle without a file (a Finder launch), a
        // moment for the system's files; otherwise a window at once.
        let bundle = std::env::current_exe()
            .is_ok_and(|p| p.to_string_lossy().contains(".app/Contents/MacOS/"));
        let started_with_file = path.is_some() || !bundle;
        if started_with_file {
            let restore = path.is_none();
            workspace::open_window(path, shared.clone(), cx);
            if restore {
                workspace::restore_last_session(&shared, cx);
            }
            shared
                .bus
                .borrow_mut()
                .emit(&kalem_core::events::Event::AppReady);
        }
        cx.activate(true);
        cx.spawn(async move |cx| {
            let mut first = !started_with_file;
            let mut plugins = plugins_at_start;
            let mut shown = kalem_core::extensions::shown();
            let mut written = kalem_core::extensions::generated_writes();
            let mut marked = kalem_core::extensions::gutter_writes();
            loop {
                cx.background_executor()
                    .timer(std::time::Duration::from_millis(if first {
                        300
                    } else {
                        150
                    }))
                    .await;
                let paths: Vec<PathBuf> = opened.borrow_mut().drain(..).collect();
                let open_empty = first && paths.is_empty();
                let ready = first;
                first = false;
                let shared = shared.clone();
                cx.update(|cx| {
                    for p in paths {
                        workspace::open_path(p, shared.clone(), cx);
                    }
                    if open_empty {
                        workspace::open_window(None, shared.clone(), cx);
                        workspace::restore_last_session(&shared, cx);
                    }
                    if ready {
                        shared
                            .bus
                            .borrow_mut()
                            .emit(&kalem_core::events::Event::AppReady);
                    }
                    // The plugins' commands and keys changed: every window
                    // gets them; the commands their event handlers asked
                    // for run in the active editor.
                    let now = kalem_core::extensions::generation();
                    if now != plugins {
                        plugins = now;
                        workspace::plugins_changed(cx);
                    }
                    let runs = kalem_core::extensions::take_runs();
                    if !runs.is_empty() {
                        workspace::run_queued(runs, cx);
                    }
                    // Their questions, asked in the active editor; their
                    // status items and panels, drawn again.
                    let asked = kalem_core::extensions::take_requests();
                    if !asked.is_empty() {
                        workspace::ask_queued(asked, cx);
                    }
                    // Their documents, shown as last written.
                    let now = kalem_core::extensions::generated_writes();
                    if now != written {
                        written = now;
                        workspace::generated_written(cx);
                    }
                    // The marks beside the lines.
                    let now = kalem_core::extensions::gutter_writes();
                    if now != marked {
                        marked = now;
                        workspace::gutters_written(cx);
                    }
                    let now = kalem_core::extensions::shown();
                    if now != shown {
                        shown = now;
                        cx.refresh_windows();
                    }
                });
            }
        })
        .detach();
    });
}

/// The path of a `file://` URL, with `%XX` escapes decoded.
fn file_url_path(url: &str) -> Option<PathBuf> {
    let rest = url.strip_prefix("file://")?;
    let rest = rest.strip_prefix("localhost").unwrap_or(rest);
    let bytes = rest.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && let Some(h) = rest.get(i + 1..i + 3)
            && let Ok(b) = u8::from_str_radix(h, 16)
        {
            out.push(b);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).ok().map(PathBuf::from)
}

/// `text` for gpui's `shape_line`, which takes one line only (and panics
/// on a line break in debug builds): its line breaks as spaces, the byte
/// length kept, so that text runs measured over it still fit. A cell's or
/// a chart label's text from a file may hold them.
pub(crate) fn one_line(text: &str) -> gpui::SharedString {
    if text.contains(['\n', '\r']) {
        text.replace(['\n', '\r'], " ").into()
    } else {
        gpui::SharedString::from(text.to_string())
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn one_line_keeps_the_length() {
        let t = "a\r\nb\nc";
        let l = super::one_line(t);
        assert_eq!(l.as_ref(), "a  b c");
        assert_eq!(l.len(), t.len());
    }

    #[test]
    fn file_urls() {
        assert_eq!(
            super::file_url_path("file:///Users/a/My%20Notes.org"),
            Some("/Users/a/My Notes.org".into())
        );
        assert_eq!(
            super::file_url_path("file:///tmp/%C3%A7.org"),
            Some("/tmp/ç.org".into())
        );
        assert_eq!(super::file_url_path("https://x.org"), None);
    }
}
