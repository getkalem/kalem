//! A picture of a graph drawn by the graphical editor (`graph-view`,
//! plugin API 0.2.12), for a look at it without a window on the screen:
//! the editor in an off-screen window, a graph of notes opened as a
//! plugin's document, the layout settled, the window saved as a PNG.
//!
//!     cargo run -p kalem-ui --example graph_shot -- /tmp/graph.png
//!
//! macOS only (Metal renders the window); run on the main thread.

use std::rc::Rc;

use gpui::{AppContext as _, VisualTestAppContext, px, size};
use kalem_core::Request;
use kalem_core::extensions::{self as x, GeneratedSpec};
use kalem_core::graph_view::{Graph, Node};
use kalem_core::settings::Config;
use kalem_ui::theme::Theme;
use kalem_ui::workspace::{self, Workspace};

fn main() {
    let out = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "/tmp/graph.png".into());
    let mut cx = VisualTestAppContext::new(gpui_platform::current_platform(false));
    let dir = std::env::temp_dir().join(format!("kalem-graph-shot-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("notes.md");
    std::fs::write(&path, "notes\n").unwrap();
    let settings = dir.join("settings.toml");
    std::fs::write(&settings, "[ui]\nlanguage = \"en\"\n").unwrap();
    let mut shared = kalem_ui::shared_in(Config::default(), Some(settings));
    shared.html_clipboard = || None;
    let shared = Rc::new(shared);
    let window = cx
        .open_offscreen_window(size(px(1100.0), px(760.0)), |window, cx| {
            let e = kalem_ui::editor::open(Some(&path), shared, Theme::light(), cx).unwrap();
            cx.new(|cx| Workspace::new(e, window, cx))
        })
        .expect("an off-screen window");
    cx.run_until_parked();
    let n = x::open_graph(
        "shot",
        GeneratedSpec {
            id: "shot.view".into(),
            key: String::new(),
            title: "Graph View: notes".into(),
            kind: "shot-view".into(),
            language: None,
        },
        notes(),
        "notes\n".into(),
    )
    .unwrap();
    cx.update(|app| workspace::ask_queued(vec![Request::ShowGenerated(n)], app));
    cx.run_until_parked();
    // Frames until the layout settles.
    for _ in 0..400 {
        let _ = cx.update_window(window.into(), |_, window, _| window.refresh());
        cx.run_until_parked();
        let moving = window
            .update(&mut cx, |ws, _, cx| {
                ws.editor
                    .read(cx)
                    .doc
                    .graph_view
                    .as_deref()
                    .is_some_and(|v| v.moving())
            })
            .unwrap_or(false);
        if !moving {
            break;
        }
    }
    let image = cx.capture_screenshot(window.into()).expect("a picture");
    image.save(&out).unwrap();
    // Where the picture went.
    let _ = std::io::Write::write_all(&mut std::io::stdout(), format!("{out}\n").as_bytes());
    let _ = std::fs::remove_dir_all(&dir);
}

/// A graph of notes: a few pages many link to, namespaces, journals.
fn notes() -> Graph {
    let mut nodes = Vec::new();
    let mut edges: Vec<(String, String)> = Vec::new();
    let page = |id: &str, group: &str, weight: u32| Node {
        id: id.to_lowercase(),
        label: id.into(),
        group: group.into(),
        weight,
    };
    for (id, group, w) in [
        ("Kalem", "pages", 14),
        ("Org", "pages", 6),
        ("Markdown", "pages", 6),
        ("Logseq", "pages", 5),
        ("Obsidian", "pages", 5),
        ("Project/Plugins", "project", 7),
        ("Project/Plugins/Graph", "project", 6),
        ("Project/Plugins/Git", "project", 3),
        ("Project/Plugins/PTA", "project", 3),
        ("Reading", "pages", 4),
        ("Rust", "pages", 4),
        ("Emacs", "pages", 3),
    ] {
        nodes.push(page(id, group, w));
    }
    let link = |e: &mut Vec<(String, String)>, a: &str, b: &str| {
        e.push((a.to_lowercase(), b.to_lowercase()))
    };
    for b in [
        "Org",
        "Markdown",
        "Logseq",
        "Obsidian",
        "Project/Plugins",
        "Rust",
        "Emacs",
    ] {
        link(&mut edges, "Kalem", b);
    }
    link(&mut edges, "Project/Plugins", "Project/Plugins/Graph");
    link(&mut edges, "Project/Plugins", "Project/Plugins/Git");
    link(&mut edges, "Project/Plugins", "Project/Plugins/PTA");
    link(&mut edges, "Project/Plugins/Graph", "Logseq");
    link(&mut edges, "Project/Plugins/Graph", "Obsidian");
    link(&mut edges, "Org", "Emacs");
    link(&mut edges, "Reading", "Rust");
    for d in 1..=24 {
        let id = format!("Oct {d}th, 2026");
        nodes.push(page(&id, "journals", 2));
        let to = [
            "Kalem",
            "Org",
            "Project/Plugins/Graph",
            "Reading",
            "Rust",
            "Markdown",
        ][d % 6];
        link(&mut edges, &id, to);
        if d % 4 == 0 {
            link(&mut edges, &id, "Logseq");
        }
    }
    for i in 0..10 {
        let id = format!("Note {i}");
        nodes.push(page(&id, "pages", 1));
        if i % 3 != 0 {
            link(&mut edges, &id, ["Reading", "Kalem", "Markdown"][i % 3]);
        }
    }
    Graph::new(nodes, &edges, None, "shot.open".into())
}
