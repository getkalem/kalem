//! Lines a plugin's layer draws under a line that start blocks of a
//! file's outline (plugin API 0.2.18), typed into in the graphical editor
//! as that outline: Enter splits a block into a new one, Tab and
//! Shift+Tab move it a step in and out, each written into the file and
//! typed into on where it went. In a test binary of its own, as the
//! layers are the process's.

use std::path::Path;
use std::rc::Rc;
use std::sync::Arc;

use gpui::TestAppContext;
use kalem_core::layers::{
    self, Below, LayerSpec, LineSource, Outcome, Outline, Overlays, VirtualLine,
};
use kalem_core::settings::Config;
use kalem_core::view::Style;
use kalem_ui::theme::Theme;
use kalem_ui::workspace::Workspace;

const NOTE: &str = "- first\n- embed here\n- third\n";

/// A page's blocks as an embed shows them, each the start of a block of
/// the page's outline (a tab a level).
fn rows(page: &Path) -> Vec<VirtualLine> {
    let file = std::fs::read_to_string(page).unwrap_or_default();
    let lines: Vec<&str> = file.lines().collect();
    let depth = |l: &str| l.len() - l.trim_start_matches('\t').len();
    (0..lines.len())
        .map(|i| {
            let d = depth(lines[i]);
            let end = (i + 1..lines.len())
                .find(|&j| depth(lines[j]) <= d)
                .unwrap_or(lines.len());
            let before = lines[..i].iter().rev().find(|l| depth(l) <= d);
            let text = lines[i][d + 2..].to_string();
            VirtualLine {
                indent: 2 + 2 * d,
                pieces: vec![(format!("• {text}"), Style::default())],
                source: Some(LineSource {
                    path: page.to_path_buf(),
                    line: i,
                    column: d + 2,
                    text,
                    outline: Some(Outline {
                        end,
                        bullet: format!("{}- ", "\t".repeat(d)),
                        step: "\t".into(),
                        indent: before.is_some_and(|l| depth(l) == d),
                        outdent: d > 0,
                    }),
                }),
            }
        })
        .collect()
}

#[gpui::test]
fn blocks_split_and_moved_where_an_embed_shows_them(cx: &mut TestAppContext) {
    let dir = std::env::temp_dir().join(format!("kalem-ui-rows-outline-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("notes.marker"), "").unwrap();
    let path = dir.join("n.md");
    std::fs::write(&path, NOTE).unwrap();
    let page = dir.join("page.md");
    std::fs::write(&page, "- one\n\t- child\n- two\n").unwrap();
    let settings = dir.join("settings.toml");
    std::fs::write(&settings, "[ui]\nlanguage = \"en\"\n").unwrap();
    kalem_core::extensions::name_folder(dir.clone());
    layers::register(LayerSpec {
        plugin: "test".into(),
        id: "notes".into(),
        markers: vec!["notes.marker".into()],
        modes: vec!["markdown".into()],
        open: None,
    });
    let embedded = page.clone();
    layers::set_provider(Arc::new(move |_, _, text: &str| {
        Outcome::Done(Overlays {
            below: vec![Below {
                line: text.find("embed").unwrap_or(0),
                lines: rows(&embedded),
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
    let editor = ws.read_with(cx, |ws, _| ws.editor.clone());
    editor.update(cx, |e, cx| {
        e.doc.move_cursor(NOTE.find("embed").unwrap(), false);
        e.after_change(cx);
    });
    cx.dispatch_action(kalem_ui::editor::RunCommand::new("view.editRow"));
    let editing = |cx: &mut gpui::VisualTestContext| {
        editor.read_with(cx, |e, _| e.doc.row_edit.as_ref().map(|r| r.input.clone()))
    };
    let page_text = || std::fs::read_to_string(&page).unwrap();
    // A few of the editor's ticks (every 50 ms) to find the block where
    // it went; a list background work offers in them (Kalem's suggestions
    // for the folder) left aside.
    let settle = |cx: &mut gpui::VisualTestContext| {
        for _ in 0..5 {
            editor.update(cx, |e, cx| {
                e.palette = None;
                cx.notify();
            });
            cx.run_until_parked();
            cx.executor()
                .advance_clock(std::time::Duration::from_millis(60));
            cx.run_until_parked();
        }
    };
    assert_eq!(editing(cx).as_deref(), Some("one"));
    // Enter at the end: a new block after "one" and its child, typed into.
    cx.simulate_keystrokes("enter");
    settle(cx);
    assert_eq!(page_text(), "- one\n\t- child\n- \n- two\n");
    assert_eq!(editing(cx).as_deref(), Some(""));
    cx.simulate_input("new");
    // Down: written, on to "two"; Tab under "new", Shift+Tab back.
    cx.simulate_keystrokes("down");
    settle(cx);
    assert_eq!(page_text(), "- one\n\t- child\n- new\n- two\n");
    assert_eq!(editing(cx).as_deref(), Some("two"));
    cx.simulate_keystrokes("tab");
    settle(cx);
    assert_eq!(page_text(), "- one\n\t- child\n- new\n\t- two\n");
    assert_eq!(editing(cx).as_deref(), Some("two"));
    cx.simulate_keystrokes("shift-tab");
    settle(cx);
    assert_eq!(page_text(), "- one\n\t- child\n- new\n- two\n");
    assert_eq!(editing(cx).as_deref(), Some("two"));
    cx.simulate_keystrokes("escape");
    ws.read_with(cx, |ws, cx| {
        assert_eq!(ws.editor.read(cx).doc.text().as_str(), NOTE);
    });
    let _ = std::fs::remove_dir_all(&dir);
}
