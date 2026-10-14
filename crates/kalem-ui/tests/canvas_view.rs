//! A plugin's document drawn as a canvas (`canvas-view`, plugin API
//! 0.2.16) in the graphical editor: its shapes where the plugin placed
//! them in place of its text, a click choosing one, Tab the next that
//! leads somewhere, the wheel zooming, Show Source showing the text. In a
//! test binary of its own, as the plugins' documents are the process's.

use std::rc::Rc;

use gpui::{Modifiers, MouseButton, TestAppContext, point, px};
use kalem_core::Request;
use kalem_core::canvas_view::{Canvas, Connector, Shape, ShapeKind};
use kalem_core::extensions::{self as x, GeneratedSpec};
use kalem_core::settings::Config;
use kalem_ui::theme::Theme;
use kalem_ui::workspace::{self, Workspace};

fn shape(id: &str, kind: ShapeKind, rect: [f32; 4], target: Option<&str>) -> Shape {
    Shape {
        id: id.into(),
        kind,
        rect,
        text: id.to_uppercase(),
        fill: None,
        stroke: None,
        dashed: false,
        target: target.map(str::to_string),
    }
}

#[gpui::test]
fn a_canvas_is_drawn_chosen_zoomed_and_shown_as_text(cx: &mut TestAppContext) {
    let dir = std::env::temp_dir().join(format!("kalem-ui-canvas-{}", std::process::id()));
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
    let canvas = Canvas::new(
        vec![
            shape(
                "frame",
                ShapeKind::Group,
                [-20.0, -20.0, 560.0, 200.0],
                None,
            ),
            shape(
                "a",
                ShapeKind::Box(6.0),
                [0.0, 0.0, 200.0, 100.0],
                Some("A"),
            ),
            shape(
                "b",
                ShapeKind::Ellipse,
                [300.0, 0.0, 200.0, 100.0],
                Some("B"),
            ),
            shape(
                "pen",
                ShapeKind::Stroke(vec![[0.0, 0.0], [40.0, 20.0], [80.0, 0.0]]),
                [0.0, 130.0, 80.0, 20.0],
                None,
            ),
        ],
        vec![(
            Some("a".into()),
            Some("b".into()),
            Connector {
                from: None,
                to: None,
                start: [0.0, 0.0],
                end: [0.0, 0.0],
                label: "links".into(),
                arrow_start: false,
                arrow_end: true,
                stroke: None,
                dashed: true,
            },
        )],
        "canvastest.open".into(),
    );
    let spec = GeneratedSpec {
        id: "canvastest.board".into(),
        key: "k".into(),
        title: "Board".into(),
        kind: "canvastest-board".into(),
        language: None,
    };
    // A command of another plugin's is refused.
    let mut other = canvas.clone();
    other.command = "elsewhere.open".into();
    assert!(x::open_canvas("canvastest", spec.clone(), other, String::new()).is_err());
    let n = x::open_canvas("canvastest", spec, canvas, "A\nB\n".into()).unwrap();
    cx.cx
        .update(|app| workspace::ask_queued(vec![Request::ShowGenerated(n)], app));
    cx.run_until_parked();
    cx.refresh().unwrap();
    cx.run_until_parked();
    // Drawn in place of the text, fitted to the window.
    let area = cx.debug_bounds("canvas-view").expect("the canvas's area");
    let view = |cx: &mut gpui::VisualTestContext| {
        ws.read_with(cx, |ws, cx| {
            let v = ws
                .editor
                .read(cx)
                .doc
                .canvas_view
                .clone()
                .expect("a canvas");
            (v.center, v.zoom, v.selected, v.target().map(str::to_string))
        })
    };
    let (center, zoom, ..) = view(cx);
    assert_eq!(center, [260.0, 80.0]);
    assert!(zoom > 0.5, "{zoom}");
    // A click on the box chooses it; on the frame where nothing else is,
    // the frame.
    let size = [f32::from(area.size.width), f32::from(area.size.height)];
    let on = |p: [f32; 2]| {
        let s = [
            (p[0] - center[0]) * zoom + size[0] / 2.0,
            (p[1] - center[1]) * zoom + size[1] / 2.0,
        ];
        area.origin + point(px(s[0]), px(s[1]))
    };
    let none = Modifiers::default();
    cx.simulate_mouse_down(on([100.0, 50.0]), MouseButton::Left, none);
    cx.simulate_mouse_up(on([100.0, 50.0]), MouseButton::Left, none);
    assert_eq!(view(cx).3.as_deref(), Some("A"));
    cx.simulate_mouse_down(on([250.0, 150.0]), MouseButton::Left, none);
    cx.simulate_mouse_up(on([250.0, 150.0]), MouseButton::Left, none);
    assert_eq!(view(cx).2, Some(0));
    // Tab: the shapes that lead somewhere, in reading order.
    cx.simulate_keystrokes("tab");
    assert_eq!(view(cx).3.as_deref(), Some("A"));
    cx.simulate_keystrokes("tab");
    assert_eq!(view(cx).3.as_deref(), Some("B"));
    // `+` zooms; Show Source shows the text.
    cx.simulate_keystrokes("+");
    assert!(view(cx).1 > zoom);
    cx.cx
        .update(|app| workspace::ask_queued(vec![Request::ToggleSource], app));
    cx.run_until_parked();
    cx.refresh().unwrap();
    cx.run_until_parked();
    assert!(cx.debug_bounds("canvas-view").is_none());
    let text = ws.read_with(cx, |ws, cx| {
        ws.editor.read(cx).doc.text().as_str().to_string()
    });
    assert_eq!(text, "A\nB\n");
    let _ = std::fs::remove_dir_all(&dir);
}
