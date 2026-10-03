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

    // Every column fitted to its text, in the grid's font.
    e.update_in(cx, |e, window, cx| {
        e.run_command(
            "viewer.grid.autofitColumns",
            serde_json::json!({}),
            window,
            cx,
        )
    });
    cx.run_until_parked();
    let widths = e.update(cx, |e, _| {
        e.doc
            .viewer
            .as_deref_mut()
            .unwrap()
            .grid_layout()
            .unwrap()
            .widths
    });
    assert!(widths[0] < 18.0 && widths[0] > 3.0, "{widths:?}");

    // Column B's edge dragged 60 pixels right: wider, written, undone.
    let before = e.update(cx, |e, _| e.doc.viewer.as_deref_mut().unwrap().col_width(1));
    let edge = cx
        .debug_bounds("viewer-grid-edge-1")
        .expect("column B's edge");
    let at = edge.center();
    let to = at + gpui::point(gpui::px(60.), gpui::px(0.));
    cx.simulate_mouse_down(at, gpui::MouseButton::Left, gpui::Modifiers::none());
    cx.simulate_mouse_move(to, Some(gpui::MouseButton::Left), gpui::Modifiers::none());
    cx.simulate_mouse_up(to, gpui::MouseButton::Left, gpui::Modifiers::none());
    cx.run_until_parked();
    let after = e.update(cx, |e, _| e.doc.viewer.as_deref_mut().unwrap().col_width(1));
    assert!(after > before + 3.0, "{before} → {after}");
    e.update_in(cx, |e, window, cx| {
        e.run_command("edit.undo", serde_json::json!({}), window, cx)
    });
    cx.run_until_parked();
    let undone = e.update(cx, |e, _| e.doc.viewer.as_deref_mut().unwrap().col_width(1));
    assert_eq!(undone, before);

    // Row 2's edge dragged 30 pixels down: taller, written, undone.
    let before = e.update(cx, |e, _| {
        e.doc.viewer.as_deref_mut().unwrap().row_height(1)
    });
    let edge = cx
        .debug_bounds("viewer-grid-row-edge-1")
        .expect("row 2's edge");
    let at = edge.center();
    let to = at + gpui::point(gpui::px(0.), gpui::px(30.));
    cx.simulate_mouse_down(at, gpui::MouseButton::Left, gpui::Modifiers::none());
    cx.simulate_mouse_move(to, Some(gpui::MouseButton::Left), gpui::Modifiers::none());
    cx.simulate_mouse_up(to, gpui::MouseButton::Left, gpui::Modifiers::none());
    cx.run_until_parked();
    let after = e.update(cx, |e, _| {
        e.doc.viewer.as_deref_mut().unwrap().row_height(1)
    });
    assert!(after > before + 10.0, "{before} → {after}");
    e.update_in(cx, |e, window, cx| {
        e.run_command("edit.undo", serde_json::json!({}), window, cx)
    });
    cx.run_until_parked();
    let undone = e.update(cx, |e, _| {
        e.doc.viewer.as_deref_mut().unwrap().row_height(1)
    });
    assert_eq!(undone, before);

    // Wrap Text on a long text: the row grows, and the cell is drawn wrapped.
    e.update_in(cx, |e, window, cx| {
        e.run_command(
            "viewer.grid.setCell",
            serde_json::json!({ "row": 8, "col": 0, "value": "Paid on the first of every month, by bank transfer" }),
            window,
            cx,
        );
        e.doc.viewer.as_deref_mut().unwrap().grid_move_to(8, 0);
        e.run_command("viewer.grid.wrapText", serde_json::json!({}), window, cx)
    });
    cx.run_until_parked();
    let (wrapped, height) = e.update(cx, |e, _| {
        let v = e.doc.viewer.as_deref_mut().unwrap();
        (v.grid_cells(8..9, 0..1)[0].2.wrap, v.row_height(8))
    });
    assert!(wrapped && height > 15.0, "{wrapped} {height}");

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
