//! Lines a plugin's layer draws under a line that show a file's text
//! (plugin API 0.2.15), typed into in the graphical editor where they are
//! painted: a click starts, Enter writes what was typed into that file,
//! Escape drops it. In a test binary of its own, as the layers are the
//! process's.

use std::rc::Rc;
use std::sync::Arc;

use gpui::{Modifiers, MouseButton, TestAppContext, point, px};
use kalem_core::layers::{self, Below, LayerSpec, LineSource, Outcome, Overlays, VirtualLine};
use kalem_core::settings::Config;
use kalem_core::view::Style;
use kalem_ui::theme::Theme;
use kalem_ui::workspace::Workspace;

const NOTE: &str = "- first\n- embed here\n- third\n";

#[gpui::test]
fn a_line_under_a_line_typed_into_writes_its_file(cx: &mut TestAppContext) {
    let dir = std::env::temp_dir().join(format!("kalem-ui-rows-typed-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("notes.marker"), "").unwrap();
    let path = dir.join("n.md");
    std::fs::write(&path, NOTE).unwrap();
    let page = dir.join("page.md");
    std::fs::write(&page, "- TODO buy milk\n- other\n").unwrap();
    let settings = dir.join("settings.toml");
    std::fs::write(&settings, "[ui]\nlanguage = \"en\"\n").unwrap();
    // The folder counts with Kalem's projects: its files may be written.
    kalem_core::extensions::name_folder(dir.clone());
    layers::register(LayerSpec {
        plugin: "test".into(),
        id: "notes".into(),
        markers: vec!["notes.marker".into()],
        modes: vec!["markdown".into()],
        open: None,
    });
    // An embed of the page's first block, as the file holds it now.
    let embedded = page.clone();
    layers::set_provider(Arc::new(move |_, _, text: &str| {
        let file = std::fs::read_to_string(&embedded).unwrap_or_default();
        let first = file.lines().next().unwrap_or("").trim_start_matches("- ");
        Outcome::Done(Overlays {
            below: vec![Below {
                line: text.find("here").unwrap(),
                lines: vec![VirtualLine {
                    indent: 2,
                    pieces: vec![(format!("• {first}"), Style::default())],
                    source: Some(LineSource {
                        path: embedded.clone(),
                        line: 0,
                        column: 2,
                        text: first.to_string(),
                    }),
                }],
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
    let embed = ws
        .read_with(cx, |ws, cx| {
            ws.editor.read(cx).painted.borrow().get(&1).cloned()
        })
        .expect("the line painted");
    let editing = |cx: &mut gpui::VisualTestContext| {
        ws.read_with(cx, |ws, cx| ws.editor.read(cx).doc.row_edit.clone())
    };
    // A click on it: typed into, what the file holds there shown.
    let on = embed.below.origin + point(px(60.), embed.below.size.height / 2.);
    let none = Modifiers::default();
    cx.simulate_mouse_down(on, MouseButton::Left, none);
    cx.simulate_mouse_up(on, MouseButton::Left, none);
    let r = editing(cx).expect("typing into it");
    assert_eq!(r.input, "TODO buy milk");
    // Typed into as a one-line input; Enter writes it into the page.
    cx.simulate_keystrokes("end backspace backspace backspace backspace");
    cx.simulate_input("bread");
    assert_eq!(editing(cx).expect("still typing").input, "TODO buy bread");
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    assert!(editing(cx).is_none());
    assert_eq!(
        std::fs::read_to_string(&page).unwrap(),
        "- TODO buy bread\n- other\n"
    );
    ws.read_with(cx, |ws, cx| {
        assert_eq!(ws.editor.read(cx).doc.text().as_str(), NOTE);
    });
    // Escape drops what was typed.
    cx.refresh().unwrap();
    cx.run_until_parked();
    cx.simulate_mouse_down(on, MouseButton::Left, none);
    cx.simulate_mouse_up(on, MouseButton::Left, none);
    assert!(editing(cx).is_some());
    cx.simulate_input("!");
    cx.simulate_keystrokes("escape");
    assert!(editing(cx).is_none());
    assert_eq!(
        std::fs::read_to_string(&page).unwrap(),
        "- TODO buy bread\n- other\n"
    );
    let _ = std::fs::remove_dir_all(&dir);
}
