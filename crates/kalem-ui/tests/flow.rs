//! A document of flowing text (plugin API 0.2.7) in the graphical editor:
//! the fake document of `tests/plugins/flowdoc`, natively, opened from a
//! file, its lines drawn with the plugin's look, typed into through the
//! plugin.

#[path = "../../../tests/plugins/flowdoc/src/lib.rs"]
#[allow(dead_code, unreachable_pub, missing_debug_implementations)]
mod flowdoc;

use std::rc::Rc;
use std::sync::Arc;

use gpui::TestAppContext;
use kalem_core::DocumentMode;
use kalem_core::settings::Config;
use kalem_ui::theme::Theme;
use kalem_ui::workspace::Workspace;

#[gpui::test]
fn a_flowing_document_is_drawn_and_typed_into(cx: &mut TestAppContext) {
    kalem_core::viewer::register(Arc::new(flowdoc::Flows));
    let dir = std::env::temp_dir().join(format!("kalem-ui-flow-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("notes.org"), "* Notes\n").unwrap();
    let file = dir.join("a.flow");
    // The NUL makes the file binary: Kalem asks its viewers for it.
    std::fs::write(
        &file,
        b"# Title\nPlain and *bold* text^\n- an item\n|a|b|\n---\nThe note.\n\0\n",
    )
    .unwrap();
    let settings = dir.join("settings.toml");
    std::fs::write(&settings, "[ui]\nlanguage = \"en\"\n").unwrap();
    let mut shared = kalem_ui::shared_in(Config::default(), Some(settings));
    shared.projects = std::cell::RefCell::new(kalem_core::projects::ProjectState::load(Some(
        dir.join("projects.toml"),
    )));
    let shared = Rc::new(shared);
    let notes = dir.join("notes.org");
    let (ws, cx) = cx.add_window_view(|window, cx| {
        let e = kalem_ui::editor::open(Some(&notes), shared, Theme::light(), cx).unwrap();
        window.focus(&gpui::Focusable::focus_handle(e.read(cx), cx), cx);
        Workspace::new(e, window, cx)
    });
    cx.run_until_parked();
    ws.update_in(cx, |ws, window, cx| ws.open(&file, None, window, cx));
    cx.run_until_parked();
    let (mode, heading, bold, label, row) = ws.read_with(cx, |ws, cx| {
        let e = ws.editor.read(cx);
        let v1 = e.line_view(1);
        (
            e.doc.meta.mode.clone(),
            e.line_view(0).heading,
            v1.runs.iter().any(|r| r.text == "bold" && r.style.bold),
            e.line_view(2).display(),
            e.line_view(3).display(),
        )
    });
    assert_eq!(mode, DocumentMode::Flow);
    assert_eq!(heading, 1);
    assert!(bold);
    assert_eq!(label, "• an item");
    assert_eq!(row, "▏a │ b");
    // Typed at the start of the second paragraph: the plugin's edit.
    ws.update_in(cx, |ws, _window, cx| {
        ws.editor.update(cx, |e, _| {
            let at = e.doc.text().as_str().find("Plain").unwrap();
            e.doc.move_cursor(at, false);
        })
    });
    cx.simulate_input("Very ");
    cx.run_until_parked();
    let (text, modified) = ws.read_with(cx, |ws, cx| {
        let e = ws.editor.read(cx);
        (e.doc.text().as_str().to_string(), e.doc.is_modified())
    });
    assert!(text.contains("Very Plain and bold"), "{text}");
    assert!(modified);
    let _ = std::fs::remove_dir_all(&dir);
}
