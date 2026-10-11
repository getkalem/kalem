//! Lines a plugin's layer draws under a line (plugin API 0.2.13) in the
//! graphical editor: painted under their line's text, a click on them
//! putting the cursor at the end of their line, the arrow keys stepping
//! over them, the source view leaving them out. In a test binary of its
//! own, as the layers are the process's.

use std::rc::Rc;
use std::sync::Arc;

use gpui::{Modifiers, MouseButton, TestAppContext, point, px};
use kalem_core::layers::{self, Below, LayerSpec, Outcome, Overlays, VirtualLine};
use kalem_core::settings::Config;
use kalem_core::view::Style;
use kalem_ui::theme::Theme;
use kalem_ui::workspace::Workspace;

const TEXT: &str = "- first\n- embed here\n- third\n";

#[gpui::test]
fn lines_under_a_line_are_painted_and_stepped_over(cx: &mut TestAppContext) {
    let dir = std::env::temp_dir().join(format!("kalem-ui-layer-lines-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("notes.marker"), "").unwrap();
    let path = dir.join("n.md");
    std::fs::write(&path, TEXT).unwrap();
    let settings = dir.join("settings.toml");
    std::fs::write(&settings, "[ui]\nlanguage = \"en\"\n").unwrap();
    layers::register(LayerSpec {
        plugin: "test".into(),
        id: "notes".into(),
        markers: vec!["notes.marker".into()],
        modes: vec!["markdown".into()],
        open: None,
    });
    layers::set_provider(Arc::new(|_, _, text: &str| {
        let line = |t: &str| VirtualLine {
            indent: 2,
            pieces: vec![(t.to_string(), Style::default())],
        };
        Outcome::Done(Overlays {
            below: vec![Below {
                line: text.find("here").unwrap(),
                lines: vec![line("• child one"), line("• child two")],
            }],
            ..Overlays::default()
        })
    }));
    let mut shared = kalem_ui::shared_in(Config::default(), Some(settings));
    shared.html_clipboard = || None;
    let shared = Rc::new(shared);
    let (ws, cx) = cx.add_window_view(|window, cx| {
        let e = kalem_ui::editor::open(Some(&path), shared, Theme::light(), cx).unwrap();
        window.focus(&gpui::Focusable::focus_handle(e.read(cx), cx), cx);
        Workspace::new(e, window, cx)
    });
    cx.run_until_parked();
    cx.refresh().unwrap();
    cx.run_until_parked();
    let painted = |cx: &mut gpui::VisualTestContext, line: usize| {
        ws.read_with(cx, |ws, cx| {
            ws.editor.read(cx).painted.borrow().get(&line).cloned()
        })
    };
    // Under the second line's text, two lines high; the others have none.
    let embed = painted(cx, 1).expect("the line painted");
    let first = painted(cx, 0).expect("the first line painted");
    let third = painted(cx, 2).expect("the third line painted");
    assert_eq!(embed.below.top(), embed.bounds.bottom());
    let row = first.bounds.size.height;
    assert!(
        embed.below.size.height >= row * 1.9,
        "{:?} for rows of {row:?}",
        embed.below
    );
    assert!(third.bounds.top() >= embed.below.bottom() - px(0.5));
    assert_eq!(first.below.size.height, px(0.));
    let head = |cx: &mut gpui::VisualTestContext| {
        ws.read_with(cx, |ws, cx| ws.editor.read(cx).doc.selection.head)
    };
    // A click on them: the cursor at the end of their line.
    let on = embed.below.origin + point(px(40.), embed.below.size.height / 2.);
    let none = Modifiers::default();
    cx.simulate_mouse_down(on, MouseButton::Left, none);
    cx.simulate_mouse_up(on, MouseButton::Left, none);
    assert_eq!(head(cx), TEXT.find("\n- third").unwrap(), "click");
    // The arrow keys step over them.
    let start = embed.bounds.origin + point(px(30.), row / 2.);
    cx.simulate_mouse_down(start, MouseButton::Left, none);
    cx.simulate_mouse_up(start, MouseButton::Left, none);
    let line_of = |cx: &mut gpui::VisualTestContext| {
        let h = head(cx);
        TEXT[..h].matches('\n').count()
    };
    assert_eq!(line_of(cx), 1);
    cx.simulate_keystrokes("down");
    assert_eq!(line_of(cx), 2, "down");
    cx.simulate_keystrokes("up");
    assert_eq!(line_of(cx), 1, "up");
    // Not in the source view.
    cx.cx.update(|app| {
        kalem_ui::workspace::ask_queued(vec![kalem_core::Request::ToggleSource], app)
    });
    cx.run_until_parked();
    cx.refresh().unwrap();
    cx.run_until_parked();
    let embed = painted(cx, 1).expect("the line painted");
    assert_eq!(embed.below.size.height, px(0.));
    let _ = std::fs::remove_dir_all(&dir);
}
