//! A plugin's panel in the graphical editor (`kalem_core::extensions`,
//! D11) taking the keys, as the terminal's does: moving the focus toward
//! it from the pane beside it gives it them, the arrows choose among its
//! widgets, Enter acts on the one chosen, typing stays out of the text,
//! Escape gives them back. In a test binary of its own, as the plugins'
//! panels and events are the process's.

use std::rc::Rc;
use std::sync::Mutex;

use gpui::TestAppContext;
use kalem_core::Request;
use kalem_core::extensions::{self as x, Panel, PanelEvent, Widget, WidgetKind};
use kalem_core::settings::Config;
use kalem_ui::theme::Theme;
use kalem_ui::workspace::{self, Workspace};

/// The Word-like keys these tests type with; Vim's are the default.
fn word_keys() -> Config {
    Config::from_layers(&[(
        kalem_core::settings::Layer::User,
        None,
        "editor.keymap_profile = \"word\"\n",
    )])
}

/// What the plugin heard: each widget's key and event.
static HEARD: Mutex<Vec<(String, PanelEvent)>> = Mutex::new(Vec::new());

/// A plugin that only listens to its panel.
struct Listener;

impl x::Extensions for Listener {
    fn run(&mut self, _: &str, _: &str) -> Result<(), String> {
        Ok(())
    }
    fn event(&mut self, _: &kalem_core::events::Event) -> Option<String> {
        None
    }
    fn answer(&mut self, _: u64, _: x::Answer) {}
    fn panel_event(&mut self, _: &str, key: &str, event: &PanelEvent) {
        HEARD
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push((key.to_string(), event.clone()));
    }
    fn settings_changed(&mut self, _: &[String]) {}
    fn respond(&mut self, _: u64, _: Result<x::HttpResponse, String>) {}
    fn process_done(&mut self, _: u64, _: Result<x::ProcessExit, String>) {}
}

fn heard() -> Vec<(String, PanelEvent)> {
    std::mem::take(&mut *HEARD.lock().unwrap_or_else(|e| e.into_inner()))
}

/// Backlinks as the graph plugin gives them: a section, a page, a long
/// reference under it.
fn widgets() -> Vec<Widget> {
    let w = |key: &str, kind: WidgetKind, children: Vec<u32>| Widget {
        key: key.into(),
        kind,
        children,
    };
    let item = |label: &str, expanded: Option<bool>| WidgetKind::Item {
        label: label.into(),
        detail: None,
        expanded,
        selected: false,
    };
    vec![
        w("", WidgetKind::Column, vec![1]),
        w("1", item("Linked references", Some(true)), vec![2]),
        w("1.0", item("Oct 13th, 2026", Some(true)), vec![3]),
        w(
            "1.0.0",
            item(
                "First try of Editor on the Book's plugins chapter, read whole at last",
                None,
            ),
            Vec::new(),
        ),
    ]
}

#[gpui::test]
fn the_panel_takes_the_keys_from_the_pane_beside_it(cx: &mut TestAppContext) {
    x::install(Box::new(Listener));
    let dir = std::env::temp_dir().join(format!("kalem-ui-panel-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("note.md");
    std::fs::write(&path, "body\n").unwrap();
    let settings = dir.join("settings.toml");
    std::fs::write(&settings, "[ui]\nlanguage = \"en\"\n").unwrap();
    let mut shared = kalem_ui::shared_in(word_keys(), Some(settings));
    shared.html_clipboard = || None;
    let shared = Rc::new(shared);
    let (ws, cx) = cx.add_window_view(|window, cx| {
        let e = kalem_ui::editor::open(Some(&path), shared, Theme::light(), cx).unwrap();
        window.focus(&gpui::Focusable::focus_handle(e.read(cx), cx), cx);
        Workspace::new(e, window, cx)
    });
    cx.run_until_parked();
    x::add_panel(Panel {
        plugin: "paneltest".into(),
        id: "paneltest.refs".into(),
        title: "Backlinks".into(),
        bottom: false,
        widgets: widgets(),
    });
    cx.cx.update(|app| {
        workspace::ask_queued(
            vec![Request::PluginPanel(Some("paneltest.refs".into()))],
            app,
        )
    });
    cx.run_until_parked();
    let focus = |cx: &mut gpui::VisualTestContext| {
        ws.read_with(cx, |ws, cx| ws.editor.read(cx).plugin_keys.focus)
    };
    let text = |cx: &mut gpui::VisualTestContext| {
        ws.read_with(cx, |ws, cx| {
            ws.editor.read(cx).doc.text().as_str().to_string()
        })
    };
    let pane_focus = |dir: &str, cx: &mut gpui::VisualTestContext| {
        ws.update_in(cx, |ws, window, cx| {
            ws.editor.update(cx, |e, cx| {
                e.run_command("pane.focus", serde_json::json!({ "dir": dir }), window, cx)
            })
        });
        cx.run_until_parked();
    };
    assert!(!focus(cx));

    // The panel stands at the window's right: moving the focus right from
    // the only pane gives it the keys, its first entry chosen.
    pane_focus("right", cx);
    assert!(focus(cx));
    // Down twice to the long reference; Enter acts on it.
    cx.simulate_keystrokes("down down enter");
    assert_eq!(heard(), [("1.0.0".to_string(), PanelEvent::Clicked)]);
    // Up and Enter: the page folds.
    cx.simulate_keystrokes("up enter");
    assert_eq!(heard(), [("1.0".to_string(), PanelEvent::Expanded(false))]);
    // Typing does not reach the text while the panel has the keys.
    cx.simulate_keystrokes("x");
    assert_eq!(text(cx), "body\n");
    // Escape gives them back: typing goes in the text again.
    cx.simulate_keystrokes("escape");
    assert!(!focus(cx));
    cx.simulate_keystrokes("x");
    assert_eq!(text(cx), "xbody\n");
    // Moving the focus right and then left does the same.
    pane_focus("right", cx);
    assert!(focus(cx));
    pane_focus("left", cx);
    assert!(!focus(cx));
    let _ = std::fs::remove_dir_all(&dir);
}
