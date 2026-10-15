//! A picture of a plugin's panel in the graphical editor (D11), for a look
//! at it without a window on the screen: the editor in an off-screen
//! window, a panel of backlinks as the graph plugin gives them (long
//! references among them) shown at its side, the window saved as a PNG.
//!
//!     cargo run -p kalem-ui --example panel_shot -- /tmp/panel.png [keys]
//!
//! With `keys`, the panel has the keys, Down pressed twice: the widget
//! chosen is framed.
//!
//! macOS only (Metal renders the window); run on the main thread.

#[cfg(target_os = "macos")]
fn main() {
    shot::main();
}

// gpui's off-screen visual context is macOS's alone: elsewhere the
// example builds, so that the workspace's targets do, and says so.
#[cfg(not(target_os = "macos"))]
#[allow(clippy::print_stderr)]
fn main() {
    eprintln!("panel_shot renders with Metal: macOS only");
}

#[cfg(target_os = "macos")]
mod shot {
    use std::rc::Rc;

    use gpui::{AppContext as _, VisualTestAppContext, px, size};
    use kalem_core::Request;
    use kalem_core::extensions::{self as x, Panel, Widget, WidgetKind};
    use kalem_core::settings::Config;
    use kalem_ui::theme::Theme;
    use kalem_ui::workspace::{self, Workspace};

    pub(super) fn main() {
        let out = std::env::args()
            .nth(1)
            .unwrap_or_else(|| "/tmp/panel.png".into());
        let mut cx = VisualTestAppContext::new(gpui_platform::current_platform(false));
        let dir = std::env::temp_dir().join(format!("kalem-panel-shot-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("Kalem.md");
        std::fs::write(
            &path,
            "- Kalem opens Org and Markdown files as they are.\n- Its plugins are listed on [[Project/Plugins]].\n",
        )
        .unwrap();
        let settings = dir.join("settings.toml");
        std::fs::write(&settings, "[ui]\nlanguage = \"en\"\n").unwrap();
        let mut shared = kalem_ui::shared_in(Config::default(), Some(settings));
        shared.html_clipboard = || None;
        let shared = Rc::new(shared);
        let window = cx
            .open_offscreen_window(size(px(1100.0), px(700.0)), |window, cx| {
                let e = kalem_ui::editor::open(Some(&path), shared, Theme::light(), cx).unwrap();
                cx.new(|cx| Workspace::new(e, window, cx))
            })
            .expect("an off-screen window");
        cx.run_until_parked();
        x::add_panel(Panel {
            plugin: "shot".into(),
            id: "shot.backlinks".into(),
            title: "Backlinks".into(),
            bottom: false,
            widgets: backlinks(),
        });
        cx.update(|app| {
            workspace::ask_queued(
                vec![Request::PluginPanel(Some("shot.backlinks".into()))],
                app,
            )
        });
        cx.run_until_parked();
        if std::env::args().nth(2).as_deref() == Some("keys") {
            let _ = window.update(&mut cx, |ws, window, cx| {
                ws.editor.update(cx, |e, cx| {
                    e.focus_plugin_panel(cx);
                    let down = gpui::Keystroke::parse("down").unwrap();
                    e.plugin_panel_key(&down, window, cx);
                    e.plugin_panel_key(&down, window, cx);
                });
            });
            cx.run_until_parked();
        }
        for _ in 0..3 {
            let _ = cx.update_window(window.into(), |_, window, _| window.refresh());
            cx.run_until_parked();
        }
        let image = cx.capture_screenshot(window.into()).expect("a picture");
        image.save(&out).unwrap();
        // Where the picture went.
        let _ = std::io::Write::write_all(&mut std::io::stdout(), format!("{out}\n").as_bytes());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The backlinks of the demo graph's Kalem page: a section, the pages
    /// under it, their references under them, one longer than four lines.
    fn backlinks() -> Vec<Widget> {
        let w = |key: &str, kind: WidgetKind, children: Vec<u32>| Widget {
            key: key.into(),
            kind,
            children,
        };
        let item = |label: &str, detail: Option<&str>, expanded: Option<bool>| WidgetKind::Item {
            label: label.into(),
            detail: detail.map(str::to_string),
            expanded,
            selected: false,
        };
        let mut out = vec![
            w("", WidgetKind::Column, vec![1, 2]),
            w("page", item("Kalem", Some("Logseq"), None), Vec::new()),
            w(
                "1",
                item("Linked references", Some("4"), Some(true)),
                Vec::new(),
            ),
        ];
        let pages = [
            (
                "Oct 13th, 2026",
                vec!["First try of Editor on the Book's plugins chapter"],
            ),
            (
                "Project/Plugins/Graph",
                vec![
                    "Logseq graphs and Obsidian vaults inside Kalem, as they are: Markdown and Org.",
                ],
            ),
            (
                "Rust",
                vec!["Kalem and its plugins are written in Rust; the plugins run as WebAssembly."],
            ),
            (
                "Long",
                vec![
                    "A reference far longer than the panel's few lines allow, so that its end is cut and an ellipsis shows where it stops, however wide the panel is, which it never is this much.",
                ],
            ),
        ];
        let mut sections = Vec::new();
        for (g, (title, refs)) in pages.into_iter().enumerate() {
            sections.push(out.len() as u32);
            let at = out.len();
            out.push(w(
                &format!("1.{g}"),
                item(title, Some(&refs.len().to_string()), Some(true)),
                Vec::new(),
            ));
            for (i, r) in refs.into_iter().enumerate() {
                let n = out.len() as u32;
                out[at].children.push(n);
                out.push(w(&format!("1.{g}.{i}"), item(r, None, None), Vec::new()));
            }
        }
        out[2].children = sections;
        out
    }
}
