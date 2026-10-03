//! An Excel workbook in the graphical editor through the xlsx plugin
//! (T3.7.4): the sheet drawn as a grid, the cursor's keys, Enter opening
//! the cell's text for editing, a cell set, copy, the next sheet.

use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use gpui::{Entity, TestAppContext, VisualTestContext};
use kalem_core::DocumentMode;
use kalem_core::settings::Config;
use kalem_ui::theme::Theme;
use kalem_ui::workspace::Workspace;

fn open(
    cx: &mut TestAppContext,
) -> (
    Entity<Workspace>,
    std::path::PathBuf,
    &mut VisualTestContext,
) {
    kalem_core::viewer::register(Arc::new(kalem_plugin_xlsx::XlsxViewer));
    static N: AtomicUsize = AtomicUsize::new(0);
    let n = N.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("kalem-ui-xlsx-{}-{n}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let data = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../kalem-tui/tests/data/budget.xlsx");
    std::fs::copy(data, dir.join("budget.xlsx")).unwrap();
    std::fs::write(dir.join("notes.org"), "* Notes\n").unwrap();
    let mut shared = kalem_ui::shared(Config::default());
    shared.html_clipboard = || None;
    shared.settings_path = Some(dir.join("settings.toml"));
    shared.projects = std::cell::RefCell::new(kalem_core::projects::ProjectState::load(Some(
        dir.join("projects.toml"),
    )));
    let shared = Rc::new(shared);
    let path = dir.join("notes.org");
    let (ws, vcx) = cx.add_window_view(|window, cx| {
        let e = kalem_ui::editor::open(Some(&path), shared, Theme::light(), cx).unwrap();
        window.focus(&gpui::Focusable::focus_handle(e.read(cx), cx), cx);
        Workspace::new(e, window, cx)
    });
    vcx.run_until_parked();
    let book = dir.join("budget.xlsx");
    ws.update_in(vcx, |ws, window, cx| ws.open(&book, None, window, cx));
    vcx.run_until_parked();
    (ws, dir, vcx)
}

fn status(ws: &Entity<Workspace>, cx: &mut VisualTestContext) -> String {
    let e = ws.read_with(cx, |ws, _| ws.editor.clone());
    e.update(cx, |e, _| {
        e.doc
            .viewer
            .as_deref_mut()
            .map(|v| v.status())
            .unwrap_or_default()
    })
}

#[gpui::test]
fn a_workbook_opens_as_a_grid(cx: &mut TestAppContext) {
    let (ws, dir, cx) = open(cx);
    let mode = ws.read_with(cx, |ws, cx| ws.editor.read(cx).doc.meta.mode.clone());
    assert_eq!(mode, DocumentMode::Viewer);
    assert!(
        cx.debug_bounds("viewer-grid").is_some(),
        "the grid is drawn"
    );
    assert!(cx.debug_bounds("viewer").is_none(), "not as a picture");
    assert!(
        cx.debug_bounds("viewer-grid-formula").is_some(),
        "the formula bar"
    );
    assert!(
        status(&ws, cx).starts_with("Budget · A1"),
        "{}",
        status(&ws, cx)
    );

    cx.simulate_keystrokes("down right");
    cx.run_until_parked();
    assert!(status(&ws, cx).contains("B2"), "{}", status(&ws, cx));

    // Edit offers the cell as entered.
    let e = ws.read_with(cx, |ws, _| ws.editor.clone());
    e.update_in(cx, |e, window, cx| {
        e.run_command("viewer.grid.edit", serde_json::json!({}), window, cx)
    });
    cx.run_until_parked();
    let direct = e.read_with(cx, |e, _| e.palette.as_ref().map(|p| p.input.clone()));
    assert_eq!(direct.as_deref(), Some("1200"));
    cx.simulate_keystrokes("escape");
    cx.run_until_parked();
    // And the key.
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    let offered = e.read_with(cx, |e, _| e.palette.as_ref().map(|p| p.input.clone()));
    assert_eq!(offered.as_deref(), Some("1200"));
    cx.simulate_keystrokes("escape");
    cx.run_until_parked();

    // A cell set: its formulas follow.
    e.update_in(cx, |e, window, cx| {
        e.run_command(
            "viewer.grid.setCell",
            serde_json::json!({ "row": 1, "col": 1, "value": "1300" }),
            window,
            cx,
        )
    });
    cx.run_until_parked();
    let d2 = e.update(cx, |e, _| {
        let v = e.doc.viewer.as_deref_mut().unwrap();
        v.grid_cells(1..2, 3..4)
            .into_iter()
            .next()
            .map(|c| c.2.text)
    });
    assert_eq!(d2.as_deref(), Some("2,500.00"));
    assert!(status(&ws, cx).contains("B3"), "{}", status(&ws, cx));

    // Copy copies the cell's text.
    e.update_in(cx, |e, window, cx| {
        e.run_command("edit.copy", serde_json::json!({}), window, cx)
    });
    cx.run_until_parked();
    let copied = cx.read_from_clipboard().and_then(|c| c.text());
    assert_eq!(copied.as_deref(), Some("431.50"));

    // The next sheet.
    let primary = if cfg!(target_os = "macos") {
        "cmd"
    } else {
        "ctrl"
    };
    cx.simulate_keystrokes(&format!("{primary}-pagedown"));
    cx.run_until_parked();
    assert!(status(&ws, cx).starts_with("Dates"), "{}", status(&ws, cx));
    let _ = std::fs::remove_dir_all(dir);
}
