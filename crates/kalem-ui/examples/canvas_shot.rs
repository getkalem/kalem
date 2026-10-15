//! A picture of a canvas drawn by the graphical editor (`canvas-view`,
//! plugin API 0.2.16), for a look at it without a window on the screen:
//! the editor in an off-screen window, a whiteboard opened as a plugin's
//! document, the window saved as a PNG.
//!
//!     cargo run -p kalem-ui --example canvas_shot -- /tmp/canvas.png
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
    eprintln!("canvas_shot renders with Metal: macOS only");
}

#[cfg(target_os = "macos")]
mod shot {
    use std::rc::Rc;

    use gpui::{AppContext as _, VisualTestAppContext, px, size};
    use kalem_core::Request;
    use kalem_core::canvas_view::{Canvas, Color, Connector, Shape, ShapeKind};
    use kalem_core::extensions::{self as x, GeneratedSpec};
    use kalem_core::settings::Config;
    use kalem_ui::theme::Theme;
    use kalem_ui::workspace::{self, Workspace};

    pub(super) fn main() {
        let out = std::env::args()
            .nth(1)
            .unwrap_or_else(|| "/tmp/canvas.png".into());
        let mut cx = VisualTestAppContext::new(gpui_platform::current_platform(false));
        let dir = std::env::temp_dir().join(format!("kalem-canvas-shot-{}", std::process::id()));
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
        let n = x::open_canvas(
            "shot",
            GeneratedSpec {
                id: "shot.board".into(),
                key: String::new(),
                title: "Whiteboard: Design".into(),
                kind: "shot-board".into(),
                language: None,
            },
            board(),
            "board\n".into(),
        )
        .unwrap();
        cx.update(|app| workspace::ask_queued(vec![Request::ShowGenerated(n)], app));
        cx.run_until_parked();
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

    /// A whiteboard as Logseq users draw one: a frame, boxes, an ellipse,
    /// text, a pen's line, a picture that cannot be read, connectors; a
    /// box's text large and a note's small (API 0.2.19).
    fn board() -> Canvas {
        let shape = |id: &str, kind: ShapeKind, rect: [f32; 4], text: &str| Shape {
            id: id.into(),
            kind,
            rect,
            text: text.into(),
            fill: None,
            stroke: None,
            dashed: false,
            target: Some(id.into()),
            text_size: None,
        };
        let line = |label: &str, dashed: bool| Connector {
            from: None,
            to: None,
            start: [0.0, 0.0],
            end: [0.0, 0.0],
            label: label.into(),
            arrow_start: false,
            arrow_end: true,
            stroke: None,
            dashed,
        };
        Canvas::new(
            vec![
                Shape {
                    stroke: Some(Color::Blue),
                    ..shape(
                        "frame",
                        ShapeKind::Group,
                        [-40.0, -60.0, 980.0, 520.0],
                        "The graph plugin",
                    )
                },
                Shape {
                    fill: Some(Color::Yellow),
                    stroke: Some(Color::Orange),
                    ..shape(
                        "plans",
                        ShapeKind::Box(8.0),
                        [0.0, 0.0, 240.0, 110.0],
                        "Plans for the launch on Reddit",
                    )
                },
                Shape {
                    text_size: Some(40.0),
                    ..shape(
                        "kalem",
                        ShapeKind::Box(8.0),
                        [420.0, 0.0, 300.0, 140.0],
                        "Kalem",
                    )
                },
                Shape {
                    fill: Some(Color::Green),
                    ..shape(
                        "idea",
                        ShapeKind::Ellipse,
                        [120.0, 250.0, 220.0, 120.0],
                        "An idea",
                    )
                },
                Shape {
                    text_size: Some(12.0),
                    ..shape(
                        "note",
                        ShapeKind::Text,
                        [450.0, 220.0, 360.0, 60.0],
                        "Whiteboards are drawn where Logseq places their shapes",
                    )
                },
                Shape {
                    stroke: Some(Color::Red),
                    ..shape(
                        "pen",
                        ShapeKind::Stroke(vec![
                            [0.0, 40.0],
                            [30.0, 0.0],
                            [60.0, 40.0],
                            [90.0, 0.0],
                            [120.0, 40.0],
                        ]),
                        [760.0, 330.0, 120.0, 40.0],
                        "",
                    )
                },
                shape(
                    "pic",
                    ShapeKind::Image("/nowhere/sketch.png".into()),
                    [450.0, 320.0, 200.0, 110.0],
                    "",
                ),
            ],
            vec![
                (
                    Some("plans".into()),
                    Some("kalem".into()),
                    line("leads to", false),
                ),
                (Some("plans".into()), Some("idea".into()), line("", true)),
            ],
            "shot.open".into(),
        )
    }
}
