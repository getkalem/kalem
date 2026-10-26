//! A plugin's document drawn as a graph (`graph-view`, plugin API 0.2.12)
//! in the graphical editor: drawn in place of its text, a click choosing a
//! node, Tab choosing the next, the mouse panning and zooming, Show Source
//! showing the text. In a test binary of its own, as the plugins'
//! documents are the process's.

use std::rc::Rc;

use gpui::{Modifiers, MouseButton, TestAppContext, point, px};
use kalem_core::Request;
use kalem_core::extensions::{self as x, GeneratedSpec};
use kalem_core::graph_view::{Graph, Node};
use kalem_core::settings::Config;
use kalem_ui::theme::Theme;
use kalem_ui::workspace::{self, Workspace};

fn node(id: &str, weight: u32) -> Node {
    Node {
        id: id.into(),
        label: id.to_uppercase(),
        group: "pages".into(),
        weight,
    }
}

#[gpui::test]
fn a_graph_is_drawn_chosen_panned_and_shown_as_text(cx: &mut TestAppContext) {
    let dir = std::env::temp_dir().join(format!("kalem-ui-graph-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("notes.md");
    std::fs::write(&path, "notes\n").unwrap();
    let settings = dir.join("settings.toml");
    std::fs::write(&settings, "[ui]\nlanguage = \"en\"\n").unwrap();
    let mut shared = kalem_ui::shared_in(Config::default(), Some(settings));
    shared.html_clipboard = || None;
    shared.projects = std::cell::RefCell::new(kalem_core::projects::ProjectState::load(Some(
        dir.join("projects.toml"),
    )));
    let shared = Rc::new(shared);
    let (ws, cx) = cx.add_window_view(|window, cx| {
        let e = kalem_ui::editor::open(Some(&path), shared, Theme::light(), cx).unwrap();
        window.focus(&gpui::Focusable::focus_handle(e.read(cx), cx), cx);
        Workspace::new(e, window, cx)
    });
    cx.run_until_parked();
    let graph = Graph::new(
        vec![node("a", 4), node("b", 1), node("c", 1)],
        &[("a".into(), "b".into()), ("a".into(), "c".into())],
        None,
        "graphtest.open".into(),
    );
    let spec = GeneratedSpec {
        id: "graphtest.view".into(),
        key: "k".into(),
        title: "Graph".into(),
        kind: "graphtest-view".into(),
        language: None,
    };
    // A command of another plugin's is refused.
    let mut other = graph.clone();
    other.command = "elsewhere.open".into();
    assert!(x::open_graph("graphtest", spec.clone(), other, String::new()).is_err());
    let n = x::open_graph("graphtest", spec, graph, "A\nB\nC\n".into()).unwrap();
    cx.cx
        .update(|app| workspace::ask_queued(vec![Request::ShowGenerated(n)], app));
    cx.run_until_parked();
    // Drawn, in place of the text, the layout settling over frames.
    let area = cx.debug_bounds("graph-view").expect("the graph's area");
    for _ in 0..200 {
        let moving = ws.read_with(cx, |ws, cx| {
            ws.editor
                .read(cx)
                .doc
                .plugins
                .graph_view
                .as_deref()
                .is_some_and(|v| v.moving())
        });
        if !moving {
            break;
        }
        cx.refresh().unwrap();
        cx.run_until_parked();
    }
    let place = |cx: &mut gpui::VisualTestContext, i: usize| {
        ws.read_with(cx, |ws, cx| {
            let v = ws
                .editor
                .read(cx)
                .doc
                .plugins
                .graph_view
                .as_deref()
                .unwrap()
                .clone();
            let size = [f32::from(area.size.width), f32::from(area.size.height)];
            let s = v.to_screen(v.place(i), size);
            area.origin + point(px(s[0]), px(s[1]))
        })
    };
    let selected = |cx: &mut gpui::VisualTestContext| {
        ws.read_with(cx, |ws, cx| {
            ws.editor
                .read(cx)
                .doc
                .plugins
                .graph_view
                .as_deref()
                .unwrap()
                .selected
        })
    };
    // A click on a node chooses it.
    let b = place(cx, 1);
    let none = Modifiers::default();
    cx.simulate_mouse_down(b, MouseButton::Left, none);
    cx.simulate_mouse_up(b, MouseButton::Left, none);
    assert_eq!(selected(cx), Some(1));
    // Tab chooses the next by weight, Shift+Tab the one before.
    cx.simulate_keystrokes("tab");
    assert_eq!(selected(cx), Some(2));
    cx.simulate_keystrokes("shift-tab shift-tab");
    assert_eq!(selected(cx), Some(0));
    // Dragging a node moves it, the layout settling around it (the
    // handler asked for a frame outside a view's drawing, which gpui
    // does not allow: 0.6.14 to 0.6.16 crashed here).
    let b = place(cx, 1);
    cx.simulate_mouse_down(b, MouseButton::Left, none);
    let to = b + point(px(40.0), px(25.0));
    cx.simulate_mouse_move(to, Some(MouseButton::Left), none);
    cx.simulate_mouse_up(to, MouseButton::Left, none);
    cx.run_until_parked();
    let moved = place(cx, 1) - b;
    assert!(f32::from(moved.x) > 20.0, "{moved:?}");
    // Dragging the background pans: node `a` follows the mouse.
    let a = place(cx, 0);
    let blank = area.origin + point(px(5.0), px(5.0));
    let to = blank + point(px(30.0), px(20.0));
    cx.simulate_mouse_down(blank, MouseButton::Left, none);
    cx.simulate_mouse_move(to, Some(MouseButton::Left), none);
    cx.simulate_mouse_up(to, MouseButton::Left, none);
    let moved = place(cx, 0) - a;
    assert!(
        (f32::from(moved.x) - 30.0).abs() < 0.5 && (f32::from(moved.y) - 20.0).abs() < 0.5,
        "{moved:?}"
    );
    // `+` zooms in.
    let zoom = |cx: &mut gpui::VisualTestContext| {
        ws.read_with(cx, |ws, cx| {
            ws.editor
                .read(cx)
                .doc
                .plugins
                .graph_view
                .as_deref()
                .unwrap()
                .zoom
        })
    };
    let z = zoom(cx);
    cx.simulate_keystrokes("+");
    assert!(zoom(cx) > z);
    // Show Source: the text, not the drawing.
    cx.cx
        .update(|app| workspace::ask_queued(vec![Request::ToggleSource], app));
    cx.run_until_parked();
    assert!(cx.debug_bounds("graph-view").is_none());
    let text = ws.read_with(cx, |ws, cx| {
        ws.editor.read(cx).doc.text().as_str().to_string()
    });
    assert_eq!(text, "A\nB\nC\n");
    // Written again: the same document drawn anew (the layout keeping
    // the nodes still there is `kalem_core::graph_view`'s to test).
    cx.cx
        .update(|app| workspace::ask_queued(vec![Request::ToggleSource], app));
    cx.run_until_parked();
    let graph = Graph::new(
        vec![node("a", 4), node("b", 1), node("d", 1)],
        &[("a".into(), "b".into()), ("a".into(), "d".into())],
        None,
        "graphtest.open".into(),
    );
    x::set_graph("graphtest", n, graph, "A\nB\nD\n".into()).unwrap();
    cx.cx.update(workspace::generated_written);
    cx.run_until_parked();
    let ids = ws.read_with(cx, |ws, cx| {
        let v = ws
            .editor
            .read(cx)
            .doc
            .plugins
            .graph_view
            .as_deref()
            .unwrap();
        v.graph()
            .nodes
            .iter()
            .map(|n| n.id.clone())
            .collect::<Vec<_>>()
    });
    assert_eq!(ids, ["a", "b", "d"]);
    assert!(cx.debug_bounds("graph-view").is_some());
    let _ = std::fs::remove_dir_all(&dir);
}
