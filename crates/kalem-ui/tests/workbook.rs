//! An Excel workbook in the graphical editor through the xlsx plugin
//! (T3.7.4): the sheet drawn as a grid, the cursor's keys, Enter opening
//! the cell's text for editing, a cell set, copy, the next sheet.

use std::rc::Rc;
use std::sync::atomic::{AtomicUsize, Ordering};

use gpui::{Entity, TestAppContext, VisualTestContext};
use kalem_core::DocumentMode;
use kalem_core::settings::Config;
use kalem_ui::theme::Theme;
use kalem_ui::workspace::Workspace;

/// A settings file of the test's own in `dir`, in English: saving or
/// reloading the settings neither touches the user's files nor switches
/// the shared interface language to the system's.
fn test_settings(dir: &std::path::Path) -> Option<std::path::PathBuf> {
    let path = dir.join("settings.toml");
    std::fs::write(&path, "[ui]\nlanguage = \"en\"\n").unwrap();
    Some(path)
}

fn open(
    cx: &mut TestAppContext,
) -> (
    Entity<Workspace>,
    std::path::PathBuf,
    &mut VisualTestContext,
) {
    kalem_core::viewer::register(kalem_components::viewer("org.kalem.xlsx").unwrap());
    static N: AtomicUsize = AtomicUsize::new(0);
    let n = N.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("kalem-ui-xlsx-{}-{n}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let data = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../kalem-tui/tests/data/budget.xlsx");
    std::fs::copy(data, dir.join("budget.xlsx")).unwrap();
    std::fs::write(dir.join("notes.org"), "* Notes\n").unwrap();
    let mut shared = kalem_ui::shared_in(Config::default(), test_settings(&dir));
    shared.html_clipboard = || None;
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
    // A range copies as tab-separated rows, as spreadsheets copy it.
    e.update_in(cx, |e, window, cx| {
        let v = e.doc.viewer.as_deref_mut().unwrap();
        v.grid_move_to(0, 0);
        v.grid_extend_to(1, 1);
        e.run_command("edit.copy", serde_json::json!({}), window, cx)
    });
    cx.run_until_parked();
    let copied = cx.read_from_clipboard().and_then(|c| c.text());
    assert_eq!(copied.as_deref(), Some("Item\tQ1\nRent\t1,300.00"));
    // Pasted at A12: the rows land in cells, the numbers as numbers.
    e.update_in(cx, |e, window, cx| {
        e.doc.viewer.as_deref_mut().unwrap().grid_move_to(11, 0);
        e.run_command("edit.paste", serde_json::json!({}), window, cx)
    });
    cx.run_until_parked();
    let (a12, b13, sel) = e.update(cx, |e, _| {
        let v = e.doc.viewer.as_deref_mut().unwrap();
        let name = v.selection_name();
        v.grid_move_to(11, 0);
        let a = v.cell_input();
        v.grid_move_to(12, 1);
        (a, v.cell_input(), name)
    });
    assert_eq!(
        (a12.as_str(), b13.as_str(), sel.as_str()),
        ("Item", "1300", "A12:B13")
    );

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

    // B3:C4 cut (a dashed frame shows it) and pasted at H10: moved, the
    // formulas that read them follow.
    e.update_in(cx, |e, window, cx| {
        let v = e.doc.viewer.as_deref_mut().unwrap();
        v.grid_move_to(2, 1);
        v.grid_extend_to(3, 2);
        e.run_command("edit.cut", serde_json::json!({}), window, cx)
    });
    cx.run_until_parked();
    assert!(
        cx.debug_bounds("viewer-grid-cut").is_some(),
        "the cut is framed"
    );
    e.update_in(cx, |e, window, cx| {
        e.doc.viewer.as_deref_mut().unwrap().grid_move_to(9, 7);
        e.run_command("edit.paste", serde_json::json!({}), window, cx)
    });
    cx.run_until_parked();
    let (b3, d3) = e.update(cx, |e, _| {
        let v = e.doc.viewer.as_deref_mut().unwrap();
        v.grid_move_to(2, 1);
        let b3 = v.cell_input();
        v.grid_move_to(2, 3);
        (b3, v.cell_input())
    });
    assert_eq!((b3.as_str(), d3.as_str()), ("", "=H10+I10"));
    e.update_in(cx, |e, window, cx| {
        e.run_command("edit.undo", serde_json::json!({}), window, cx)
    });
    cx.run_until_parked();

    // Budget!B3:D3 cut, pasted on Dates at C10: moved across sheets.
    e.update_in(cx, |e, window, cx| {
        let v = e.doc.viewer.as_deref_mut().unwrap();
        v.grid_move_to(2, 1);
        v.grid_extend_to(2, 3);
        e.run_command("edit.cut", serde_json::json!({}), window, cx);
        e.run_command("viewer.grid.nextSheet", serde_json::json!({}), window, cx);
        e.doc.viewer.as_deref_mut().unwrap().grid_move_to(9, 2);
        e.run_command("edit.paste", serde_json::json!({}), window, cx)
    });
    cx.run_until_parked();
    let e10 = e.update(cx, |e, _| {
        let v = e.doc.viewer.as_deref_mut().unwrap();
        v.grid_move_to(9, 4);
        v.cell_input()
    });
    assert_eq!(e10, "=Dates!C10+Dates!D10");
    e.update_in(cx, |e, window, cx| {
        e.run_command("edit.undo", serde_json::json!({}), window, cx);
        e.run_command(
            "viewer.grid.previousSheet",
            serde_json::json!({}),
            window,
            cx,
        )
    });
    cx.run_until_parked();
    assert!(status(&ws, cx).starts_with("Budget"), "{}", status(&ws, cx));

    // B2 to C3 selected by dragging, then merged.
    let from = cx
        .debug_bounds("viewer-grid-cell-1-1")
        .expect("B2")
        .center();
    let to = cx
        .debug_bounds("viewer-grid-cell-2-2")
        .expect("C3")
        .center();
    cx.simulate_mouse_down(from, gpui::MouseButton::Left, gpui::Modifiers::none());
    cx.simulate_mouse_move(to, Some(gpui::MouseButton::Left), gpui::Modifiers::none());
    cx.simulate_mouse_up(to, gpui::MouseButton::Left, gpui::Modifiers::none());
    cx.run_until_parked();
    assert!(status(&ws, cx).contains("B2:C3"), "{}", status(&ws, cx));
    e.update_in(cx, |e, window, cx| {
        e.run_command(
            "viewer.grid.mergeCenter",
            serde_json::json!({ "confirmed": true }),
            window,
            cx,
        )
    });
    cx.run_until_parked();
    let merged = e.update(cx, |e, _| {
        e.doc
            .viewer
            .as_deref_mut()
            .unwrap()
            .grid_layout()
            .unwrap()
            .merged
    });
    assert!(merged.contains(&[1, 1, 2, 2]), "{merged:?}");

    // The filter on, its button offering the column's values.
    e.update_in(cx, |e, window, cx| {
        e.doc.viewer.as_deref_mut().unwrap().grid_move_to(1, 0);
        e.run_command(
            "viewer.grid.toggleFilter",
            serde_json::json!({}),
            window,
            cx,
        )
    });
    cx.run_until_parked();
    let button = cx
        .debug_bounds("viewer-grid-filter-0")
        .expect("A1's filter button");
    cx.simulate_click(button.center(), gpui::Modifiers::none());
    cx.run_until_parked();
    let offered = e.read_with(cx, |e, _| e.palette.is_some());
    assert!(offered, "the column's values are offered");
    cx.simulate_keystrokes("escape");
    cx.run_until_parked();
    e.update_in(cx, |e, window, cx| {
        e.run_command(
            "viewer.grid.toggleFilter",
            serde_json::json!({}),
            window,
            cx,
        )
    });
    cx.run_until_parked();

    // A data bar and an icon set on D2:D4, drawn in the cells.
    e.update_in(cx, |e, window, cx| {
        let v = e.doc.viewer.as_deref_mut().unwrap();
        v.grid_move_to(1, 3);
        v.grid_extend_to(3, 3);
        e.run_command(
            "viewer.grid.dataBars",
            serde_json::json!({ "color": "#638EC6" }),
            window,
            cx,
        );
        e.run_command(
            "viewer.grid.iconSet",
            serde_json::json!({ "name": "3Arrows" }),
            window,
            cx,
        );
    });
    cx.run_until_parked();
    let bar = cx.debug_bounds("viewer-grid-bar-2-3").expect("D3's bar");
    let cell = cx.debug_bounds("viewer-grid-cell-2-3").expect("D3");
    assert!(bar.size.width > gpui::px(0.) && bar.size.width < cell.size.width);
    assert!(
        cx.debug_bounds("viewer-grid-icon-2-3").is_some(),
        "D3's icon"
    );
    e.update_in(cx, |e, window, cx| {
        e.run_command(
            "viewer.grid.clearSheetConditionalFormats",
            serde_json::json!({}),
            window,
            cx,
        )
    });
    cx.run_until_parked();
    assert!(cx.debug_bounds("viewer-grid-bar-2-3").is_none());

    // A list on A2:A5 leaving Sum out: its button on the cursor's cell
    // offers the values, and Circle Invalid Data rings Sum.
    e.update_in(cx, |e, window, cx| {
        let v = e.doc.viewer.as_deref_mut().unwrap();
        v.grid_move_to(1, 0);
        v.grid_extend_to(4, 0);
        e.run_command(
            "viewer.grid.validateList",
            serde_json::json!({ "value": "Food,Rent,Travel" }),
            window,
            cx,
        );
        e.doc.viewer.as_deref_mut().unwrap().grid_move_to(1, 0);
        e.run_command(
            "viewer.grid.circleInvalid",
            serde_json::json!({}),
            window,
            cx,
        );
    });
    cx.run_until_parked();
    assert!(
        cx.debug_bounds("viewer-grid-invalid-4-0").is_some(),
        "Sum ringed"
    );
    assert!(cx.debug_bounds("viewer-grid-invalid-1-0").is_none());
    let button = cx
        .debug_bounds("viewer-grid-list")
        .expect("the list's button");
    cx.simulate_click(button.center(), gpui::Modifiers::none());
    cx.run_until_parked();
    let offered = e.read_with(cx, |e, _| e.palette.is_some());
    assert!(offered, "the list's values are offered");
    cx.simulate_keystrokes("escape");
    cx.run_until_parked();

    // The fill handle: A8:A9 (1, 2) dragged two rows down, a series; the
    // target framed while dragging.
    e.update_in(cx, |e, window, cx| {
        e.run_command(
            "viewer.grid.setCell",
            serde_json::json!({ "row": 7, "col": 0, "value": "1" }),
            window,
            cx,
        );
        e.run_command(
            "viewer.grid.setCell",
            serde_json::json!({ "row": 8, "col": 0, "value": "2" }),
            window,
            cx,
        );
        let v = e.doc.viewer.as_deref_mut().unwrap();
        v.grid_move_to(7, 0);
        v.grid_extend_to(8, 0);
    });
    cx.run_until_parked();
    let handle = cx
        .debug_bounds("viewer-grid-fill-handle")
        .expect("the selection's fill handle");
    // Dropped on A11.
    let to = cx
        .debug_bounds("viewer-grid-cell-10-0")
        .expect("A11 in view")
        .center();
    cx.simulate_mouse_down(
        handle.center(),
        gpui::MouseButton::Left,
        gpui::Modifiers::none(),
    );
    cx.simulate_mouse_move(to, Some(gpui::MouseButton::Left), gpui::Modifiers::none());
    cx.run_until_parked();
    assert!(
        cx.debug_bounds("viewer-grid-fill-frame").is_some(),
        "the target framed"
    );
    cx.simulate_mouse_up(to, gpui::MouseButton::Left, gpui::Modifiers::none());
    cx.run_until_parked();
    let (a10, a11) = e.update(cx, |e, _| {
        let v = e.doc.viewer.as_deref_mut().unwrap();
        v.grid_move_to(9, 0);
        let a = v.cell_input();
        v.grid_move_to(10, 0);
        (a, v.cell_input())
    });
    assert_eq!((a10.as_str(), a11.as_str()), ("3", "4"));
    // The handle double-clicked: B8 "x" beside A8:A11 fills down to B11.
    e.update_in(cx, |e, window, cx| {
        e.run_command(
            "viewer.grid.setCell",
            serde_json::json!({ "row": 7, "col": 1, "value": "x 1" }),
            window,
            cx,
        );
        e.doc.viewer.as_deref_mut().unwrap().grid_move_to(7, 1);
    });
    cx.run_until_parked();
    let handle = cx.debug_bounds("viewer-grid-fill-handle").unwrap().center();
    cx.simulate_event(gpui::MouseDownEvent {
        position: handle,
        button: gpui::MouseButton::Left,
        modifiers: gpui::Modifiers::none(),
        click_count: 2,
        first_mouse: false,
    });
    cx.simulate_event(gpui::MouseUpEvent {
        position: handle,
        button: gpui::MouseButton::Left,
        modifiers: gpui::Modifiers::none(),
        click_count: 2,
    });
    cx.run_until_parked();
    let b11 = e.update(cx, |e, _| {
        let v = e.doc.viewer.as_deref_mut().unwrap();
        v.grid_move_to(10, 1);
        v.cell_input()
    });
    assert_eq!(b11, "x 4");
    e.update_in(cx, |e, window, cx| {
        e.run_command("edit.undo", serde_json::json!({}), window, cx);
        e.run_command("edit.undo", serde_json::json!({}), window, cx);
    });
    for _ in 0..3 {
        e.update_in(cx, |e, window, cx| {
            e.run_command("edit.undo", serde_json::json!({}), window, cx)
        });
    }
    cx.run_until_parked();

    // Flash Fill beside the table: G2 "Rent!" makes G3 "Food!".
    e.update_in(cx, |e, window, cx| {
        e.run_command(
            "viewer.grid.setCell",
            serde_json::json!({ "row": 1, "col": 4, "value": "Rent!" }),
            window,
            cx,
        );
        e.doc.viewer.as_deref_mut().unwrap().grid_move_to(1, 4);
        e.run_command("viewer.grid.flashFill", serde_json::json!({}), window, cx);
    });
    cx.run_until_parked();
    let e3 = e.update(cx, |e, _| {
        let v = e.doc.viewer.as_deref_mut().unwrap();
        v.grid_move_to(2, 4);
        v.cell_input()
    });
    assert_eq!(e3, "Food!");
    e.update_in(cx, |e, window, cx| {
        e.run_command("edit.undo", serde_json::json!({}), window, cx);
        e.run_command("edit.undo", serde_json::json!({}), window, cx);
    });
    cx.run_until_parked();

    // Bold with Ctrl+B, and a font size the cell is drawn at.
    e.update_in(cx, |e, _, _| {
        e.doc.viewer.as_deref_mut().unwrap().grid_move_to(3, 0)
    });
    cx.run_until_parked();
    let before = cx.debug_bounds("viewer-grid-cell-3-0").unwrap();
    // Ctrl+B, the platform's primary modifier on a Mac.
    let primary_b = if cfg!(target_os = "macos") {
        "cmd-b"
    } else {
        "ctrl-b"
    };
    cx.simulate_keystrokes(primary_b);
    e.update_in(cx, |e, window, cx| {
        e.run_command(
            "viewer.grid.fontSize",
            serde_json::json!({ "value": "20" }),
            window,
            cx,
        )
    });
    cx.run_until_parked();
    let c = e.update(cx, |e, _| {
        e.doc.viewer.as_deref_mut().unwrap().cursor_cell()
    });
    assert!(c.bold, "{c:?}");
    assert_eq!(c.font_size, Some(200));
    assert!(cx.debug_bounds("viewer-grid-cell-3-0").unwrap().size.width >= before.size.width);
    e.update_in(cx, |e, window, cx| {
        e.run_command("edit.undo", serde_json::json!({}), window, cx);
        e.run_command("edit.undo", serde_json::json!({}), window, cx);
    });
    cx.run_until_parked();

    // Shift+Space selects the row; Ctrl+Down goes to the data's end.
    cx.simulate_keystrokes("shift-space");
    cx.run_until_parked();
    let s = e.update(cx, |e, _| e.doc.viewer.as_deref_mut().unwrap().selection());
    assert_eq!((s[0], s[1], s[2]), (3, 0, 3));
    assert!(s[3] > 100, "{s:?}");
    // The row's numbers summed in the status bar, the label counted.
    assert!(
        status(&ws, cx).contains(
            "Average: 633.33 · Count: 4 · Sum: 1900 · Numerical Count: 3 · Min: 0 · Max: 950"
        ),
        "{}",
        status(&ws, cx)
    );
    e.update_in(cx, |e, _, _| {
        e.doc.viewer.as_deref_mut().unwrap().grid_move_to(0, 0)
    });
    let primary_down = if cfg!(target_os = "macos") {
        "cmd-down"
    } else {
        "ctrl-down"
    };
    cx.simulate_keystrokes(primary_down);
    cx.run_until_parked();
    let p = e.update(cx, |e, _| e.doc.viewer.as_deref_mut().unwrap().grid_pos());
    assert_eq!((p.row, p.col), (4, 0));
    e.update_in(cx, |e, _, _| {
        e.doc.viewer.as_deref_mut().unwrap().grid_move_to(3, 0)
    });
    cx.run_until_parked();

    // AutoComplete in a cell's entry: "fo" offers Food, Enter takes it;
    // Alt+Enter a line break.
    e.update_in(cx, |e, window, cx| {
        e.doc.viewer.as_deref_mut().unwrap().grid_move_to(6, 0);
        e.run_command("viewer.grid.edit", serde_json::json!({}), window, cx);
    });
    e.update(cx, |e, cx| e.panel_input("fo", cx));
    cx.run_until_parked();
    let offer = e.update(cx, |e, _| e.palette.as_ref().and_then(|p| p.offer.clone()));
    assert_eq!(offer.as_deref(), Some("Food"));
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    let a7 = e.update(cx, |e, _| {
        let v = e.doc.viewer.as_deref_mut().unwrap();
        v.grid_move_to(6, 0);
        v.cell_input()
    });
    assert_eq!(a7, "Food");
    e.update_in(cx, |e, window, cx| {
        e.doc.viewer.as_deref_mut().unwrap().grid_move_to(7, 0);
        e.run_command("viewer.grid.edit", serde_json::json!({}), window, cx);
    });
    e.update(cx, |e, cx| e.panel_input("a", cx));
    cx.simulate_keystrokes("alt-enter");
    e.update(cx, |e, cx| e.panel_input("b", cx));
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    let a8 = e.update(cx, |e, _| {
        let v = e.doc.viewer.as_deref_mut().unwrap();
        v.grid_move_to(7, 0);
        v.cell_input()
    });
    assert_eq!(a8, "a\nb");
    e.update_in(cx, |e, window, cx| {
        e.run_command("edit.undo", serde_json::json!({}), window, cx);
        e.run_command("edit.undo", serde_json::json!({}), window, cx);
        e.doc.viewer.as_deref_mut().unwrap().grid_move_to(3, 0);
    });
    cx.run_until_parked();

    // A formula: Tab completes SUM(, Up points at B6, drawn as such.
    e.update_in(cx, |e, window, cx| {
        e.doc.viewer.as_deref_mut().unwrap().grid_move_to(6, 1);
        e.run_command("viewer.grid.edit", serde_json::json!({}), window, cx);
    });
    e.update(cx, |e, cx| e.panel_input("=su", cx));
    cx.run_until_parked();
    let hint = e.update(cx, |e, _| e.palette.as_ref().and_then(|p| p.hint.clone()));
    assert!(hint.is_some_and(|h| h.starts_with("Tab: SUM(")));
    cx.simulate_keystrokes("tab up");
    cx.run_until_parked();
    let typed = e.update(cx, |e, _| e.palette.as_ref().map(|p| p.input.clone()));
    assert_eq!(typed.as_deref(), Some("=SUM(B6"));
    assert!(cx.debug_bounds("viewer-grid-pointer").is_some());
    cx.simulate_keystrokes("escape");
    cx.run_until_parked();
    assert!(cx.debug_bounds("viewer-grid-pointer").is_none());
    e.update_in(cx, |e, _, _| {
        e.doc.viewer.as_deref_mut().unwrap().grid_move_to(3, 0)
    });
    cx.run_until_parked();

    // Rows 2-4 grouped: row 5's − collapses them when clicked, + again.
    e.update_in(cx, |e, window, cx| {
        let v = e.doc.viewer.as_deref_mut().unwrap();
        v.grid_move_to(1, 0);
        v.grid_extend_to(3, 0);
        e.run_command("viewer.grid.group", serde_json::json!({}), window, cx);
    });
    cx.run_until_parked();
    let mark = cx.debug_bounds("viewer-grid-outline-4").unwrap();
    cx.simulate_click(mark.center(), gpui::Modifiers::none());
    cx.run_until_parked();
    let hidden = e.update(cx, |e, _| {
        e.doc
            .viewer
            .as_deref_mut()
            .unwrap()
            .grid_layout()
            .unwrap()
            .hidden_rows
    });
    assert_eq!(hidden, vec![1, 2, 3]);
    let mark = cx.debug_bounds("viewer-grid-outline-4").unwrap();
    cx.simulate_click(mark.center(), gpui::Modifiers::none());
    cx.run_until_parked();
    e.update_in(cx, |e, window, cx| {
        for _ in 0..3 {
            e.run_command("edit.undo", serde_json::json!({}), window, cx);
        }
        e.doc.viewer.as_deref_mut().unwrap().grid_move_to(3, 0);
    });
    cx.run_until_parked();

    // Trace Precedents of D2: arrows drawn over the grid; then removed.
    e.update_in(cx, |e, window, cx| {
        e.doc.viewer.as_deref_mut().unwrap().grid_move_to(1, 3);
        e.run_command(
            "viewer.grid.tracePrecedents",
            serde_json::json!({}),
            window,
            cx,
        );
    });
    cx.run_until_parked();
    assert!(cx.debug_bounds("viewer-grid-arrows").is_some());
    e.update_in(cx, |e, window, cx| {
        e.run_command(
            "viewer.grid.removeArrows",
            serde_json::json!({}),
            window,
            cx,
        );
        e.doc.viewer.as_deref_mut().unwrap().grid_move_to(3, 0);
    });
    cx.run_until_parked();
    assert!(cx.debug_bounds("viewer-grid-arrows").is_none());

    // A shape drawn over its cells; then undone.
    e.update_in(cx, |e, window, cx| {
        e.doc.viewer.as_deref_mut().unwrap().grid_move_to(5, 0);
        e.run_command(
            "viewer.grid.insertShape",
            serde_json::json!({ "shape": "ellipse", "value": "Hedef" }),
            window,
            cx,
        );
    });
    cx.run_until_parked();
    let shape = cx.debug_bounds("viewer-grid-drawing-0").unwrap();
    let a6 = cx.debug_bounds("viewer-grid-cell-5-0").unwrap();
    assert_eq!(shape.origin, a6.origin);
    e.update_in(cx, |e, window, cx| {
        e.run_command("edit.undo", serde_json::json!({}), window, cx);
        e.doc.viewer.as_deref_mut().unwrap().grid_move_to(3, 0);
    });
    cx.run_until_parked();
    assert!(cx.debug_bounds("viewer-grid-drawing-0").is_none());

    // A sparkline of B2:D2 drawn inside H2; then undone.
    e.update_in(cx, |e, window, cx| {
        let v = e.doc.viewer.as_deref_mut().unwrap();
        v.grid_move_to(1, 1);
        v.grid_extend_to(1, 3);
        e.run_command(
            "viewer.grid.insertSparklines",
            serde_json::json!({ "kind": "column", "value": "H2" }),
            window,
            cx,
        );
    });
    cx.run_until_parked();
    let line = cx.debug_bounds("viewer-grid-sparkline-1-7").unwrap();
    let h2 = cx.debug_bounds("viewer-grid-cell-1-7").unwrap();
    assert!(h2.contains(&line.origin) && line.size.width < h2.size.width);
    e.update_in(cx, |e, window, cx| {
        e.run_command("edit.undo", serde_json::json!({}), window, cx);
        e.doc.viewer.as_deref_mut().unwrap().grid_move_to(3, 0);
    });
    cx.run_until_parked();
    assert!(cx.debug_bounds("viewer-grid-sparkline-1-7").is_none());

    // A comment on E2: Excel's purple mark; then undone.
    e.update_in(cx, |e, window, cx| {
        e.doc.viewer.as_deref_mut().unwrap().grid_move_to(1, 4);
        e.run_command(
            "viewer.grid.newComment",
            serde_json::json!({ "value": "Kontrol" }),
            window,
            cx,
        );
    });
    cx.run_until_parked();
    assert!(cx.debug_bounds("viewer-grid-thread-1-4").is_some());
    e.update_in(cx, |e, window, cx| {
        e.run_command("edit.undo", serde_json::json!({}), window, cx);
        e.doc.viewer.as_deref_mut().unwrap().grid_move_to(3, 0);
    });
    cx.run_until_parked();
    assert!(cx.debug_bounds("viewer-grid-thread-1-4").is_none());

    // The sheets' tabs: a click on the second shows it; back with the first.
    let second = cx
        .debug_bounds("viewer-grid-tab-1")
        .expect("the second sheet's tab");
    cx.simulate_click(second.center(), gpui::Modifiers::none());
    cx.run_until_parked();
    assert_eq!(
        e.update(cx, |e, _| e.doc.viewer.as_deref().unwrap().unit),
        1
    );
    let first = cx.debug_bounds("viewer-grid-tab-0").unwrap();
    cx.simulate_click(first.center(), gpui::Modifiers::none());
    cx.run_until_parked();
    assert_eq!(
        e.update(cx, |e, _| e.doc.viewer.as_deref().unwrap().unit),
        0
    );

    // Zoom 200%: the cells twice as large; headings off: the cells start
    // at the left; Page Break Preview: the page's number over it.
    let a2 = cx.debug_bounds("viewer-grid-cell-1-0").unwrap();
    e.update_in(cx, |e, window, cx| {
        e.run_command(
            "viewer.grid.zoom",
            serde_json::json!({ "value": "200" }),
            window,
            cx,
        );
        e.run_command(
            "viewer.grid.toggleHeadings",
            serde_json::json!({}),
            window,
            cx,
        );
        e.run_command(
            "viewer.grid.pageBreakPreview",
            serde_json::json!({}),
            window,
            cx,
        );
    });
    cx.run_until_parked();
    let big = cx.debug_bounds("viewer-grid-cell-1-0").unwrap();
    assert!(big.size.height > a2.size.height * 1.6, "{a2:?} {big:?}");
    assert!(big.origin.x < a2.origin.x);
    assert!(cx.debug_bounds("viewer-grid-page-1").is_some());
    e.update_in(cx, |e, window, cx| {
        e.run_command("viewer.grid.zoom100", serde_json::json!({}), window, cx);
        e.run_command(
            "viewer.grid.toggleHeadings",
            serde_json::json!({}),
            window,
            cx,
        );
        e.run_command(
            "viewer.grid.pageBreakPreview",
            serde_json::json!({}),
            window,
            cx,
        );
    });
    cx.run_until_parked();
    assert!(cx.debug_bounds("viewer-grid-pages").is_none());
    assert_eq!(cx.debug_bounds("viewer-grid-cell-1-0").unwrap(), a2);

    // A2 dragged by its border to A8: moved there; then undone.
    e.update_in(cx, |e, _, _| {
        e.doc.viewer.as_deref_mut().unwrap().grid_move_to(1, 0);
    });
    cx.run_until_parked();
    let rent = e.update(cx, |e, _| e.doc.viewer.as_deref_mut().unwrap().cell_input());
    let edge = cx
        .debug_bounds("viewer-grid-move-edge-1")
        .expect("the border's bottom");
    let a8 = cx.debug_bounds("viewer-grid-cell-7-0").unwrap();
    cx.simulate_mouse_down(
        edge.center(),
        gpui::MouseButton::Left,
        gpui::Modifiers::none(),
    );
    cx.simulate_mouse_move(
        a8.center(),
        Some(gpui::MouseButton::Left),
        gpui::Modifiers::none(),
    );
    cx.run_until_parked();
    assert!(cx.debug_bounds("viewer-grid-move-frame").is_some());
    cx.simulate_mouse_up(
        a8.center(),
        gpui::MouseButton::Left,
        gpui::Modifiers::none(),
    );
    cx.run_until_parked();
    let (moved, left) = e.update(cx, |e, _| {
        let v = e.doc.viewer.as_deref_mut().unwrap();
        let moved = v.cell_input();
        v.grid_move_to(1, 0);
        (moved, v.cell_input())
    });
    assert_eq!((moved, left), (rent, String::new()));
    e.update_in(cx, |e, window, cx| {
        e.run_command("edit.undo", serde_json::json!({}), window, cx);
        e.doc.viewer.as_deref_mut().unwrap().grid_move_to(3, 0);
    });
    cx.run_until_parked();
    // A right click on a cell outside the selection: the cell selected,
    // its menu.
    let b5 = cx.debug_bounds("viewer-grid-cell-4-1").unwrap();
    cx.simulate_mouse_down(
        b5.center(),
        gpui::MouseButton::Right,
        gpui::Modifiers::none(),
    );
    cx.run_until_parked();
    let p = e.update(cx, |e, _| e.doc.viewer.as_deref().unwrap().grid_pos());
    assert_eq!((p.row, p.col), (4, 1));
    cx.simulate_keystrokes("escape");
    e.update_in(cx, |e, _, _| {
        e.doc.viewer.as_deref_mut().unwrap().grid_move_to(3, 0);
    });
    cx.run_until_parked();

    // Goal Seek: D2 (B2+C2) made 1000 by changing B2; then undone.
    e.update_in(cx, |e, window, cx| {
        e.run_command(
            "viewer.grid.goalSeek",
            serde_json::json!({ "set cell": "D2", "to value": "1000", "by changing cell": "B2" }),
            window,
            cx,
        );
    });
    cx.run_until_parked();
    let d2 = e.update(cx, |e, _| {
        let v = e.doc.viewer.as_deref_mut().unwrap();
        v.grid_cells(1..2, 3..4)[0].2.text.clone()
    });
    assert!(d2.replace(',', "").starts_with("1000"), "{d2}");
    e.update_in(cx, |e, window, cx| {
        e.run_command("edit.undo", serde_json::json!({}), window, cx);
        e.doc.viewer.as_deref_mut().unwrap().grid_move_to(3, 0);
    });
    cx.run_until_parked();

    // Find: the first match after the cursor, then the next with F3.
    e.update_in(cx, |e, window, cx| {
        e.run_command(
            "viewer.grid.find",
            serde_json::json!({ "value": "950" }),
            window,
            cx,
        )
    });
    cx.simulate_keystrokes("f3");
    cx.run_until_parked();
    let p = e.update(cx, |e, _| e.doc.viewer.as_deref_mut().unwrap().grid_pos());
    assert_eq!((p.row, p.col), (3, 3));
    e.update_in(cx, |e, _, _| {
        e.doc.viewer.as_deref_mut().unwrap().grid_move_to(3, 0)
    });
    cx.run_until_parked();

    // Thick outside borders, drawn along the cell's sides; then undone.
    e.update_in(cx, |e, window, cx| {
        e.run_command(
            "viewer.grid.borders",
            serde_json::json!({ "set": "thick" }),
            window,
            cx,
        )
    });
    cx.run_until_parked();
    let cell = cx.debug_bounds("viewer-grid-cell-3-0").unwrap();
    let bottom = cx.debug_bounds("viewer-grid-border-3-0-2").unwrap();
    assert_eq!(bottom.size.height, gpui::px(2.));
    assert_eq!(bottom.bottom(), cell.bottom());
    assert!(cx.debug_bounds("viewer-grid-border-3-0-3").is_some());
    e.update_in(cx, |e, window, cx| {
        e.run_command("edit.undo", serde_json::json!({}), window, cx);
    });
    cx.run_until_parked();
    assert!(cx.debug_bounds("viewer-grid-border-3-0-2").is_none());

    // Centered at the top, drawn so; then undone.
    e.update_in(cx, |e, window, cx| {
        e.run_command("viewer.grid.alignCenter", serde_json::json!({}), window, cx);
        e.run_command("viewer.grid.alignTop", serde_json::json!({}), window, cx);
    });
    cx.run_until_parked();
    let c = e.update(cx, |e, _| {
        e.doc.viewer.as_deref_mut().unwrap().cursor_cell()
    });
    assert_eq!(
        (c.align, c.valign),
        (kalem_viewer::Align::Center, kalem_viewer::VAlign::Top)
    );
    assert!(cx.debug_bounds("viewer-grid-cell-3-0").is_some());
    e.update_in(cx, |e, window, cx| {
        e.run_command("edit.undo", serde_json::json!({}), window, cx);
        e.run_command("edit.undo", serde_json::json!({}), window, cx);
    });
    cx.run_until_parked();

    // The next sheet.
    let primary = if cfg!(target_os = "macos") {
        "cmd"
    } else {
        "ctrl"
    };
    cx.simulate_keystrokes(&format!("{primary}-pagedown"));
    cx.run_until_parked();
    assert!(status(&ws, cx).starts_with("Dates"), "{}", status(&ws, cx));

    // A pivot table of the budget on a new sheet, shown; then refreshed.
    cx.simulate_keystrokes(&format!("{primary}-pageup"));
    cx.run_until_parked();
    e.update_in(cx, |e, window, cx| {
        e.doc.viewer.as_deref_mut().unwrap().grid_move_to(1, 0);
        e.run_command(
            "viewer.grid.insertPivot",
            serde_json::json!({ "rows": [0], "cols": [], "values": [[1, "sum"]], "step": "create" }),
            window,
            cx,
        );
    });
    cx.run_until_parked();
    assert!(status(&ws, cx).starts_with("Pivot1"), "{}", status(&ws, cx));
    assert!(cx.debug_bounds("viewer-grid-cell-2-0").is_some());
    e.update_in(cx, |e, window, cx| {
        e.run_command(
            "viewer.grid.refreshPivots",
            serde_json::json!({}),
            window,
            cx,
        )
    });
    cx.run_until_parked();
    let total = e.update(cx, |e, _| {
        let v = e.doc.viewer.as_deref_mut().unwrap();
        (3..20)
            .find(|&r| {
                v.grid_move_to(r, 0);
                v.cell_input() == "Grand Total"
            })
            .is_some()
    });
    assert!(total, "the pivot table's total row");

    // A chart of the budget: drawn over the cells beside the table.
    e.update_in(cx, |e, window, cx| {
        e.doc.viewer.as_deref_mut().unwrap().go_to(0);
        e.doc.viewer.as_deref_mut().unwrap().grid_move_to(1, 1);
        e.run_command(
            "viewer.grid.insertChart",
            serde_json::json!({ "kind": "pie", "title": "Rent and the rest" }),
            window,
            cx,
        );
        e.doc.viewer.as_deref_mut().unwrap().grid_move_to(0, 0);
    });
    cx.run_until_parked();
    let n = e.update(cx, |e, _| {
        e.doc.viewer.as_deref_mut().unwrap().charts().len()
    });
    assert!(
        cx.debug_bounds(Box::leak(
            format!("viewer-grid-chart-{}", n - 1).into_boxed_str()
        ))
        .is_some(),
        "the new chart drawn"
    );
    // Dragged by its body two rows down and a column right, then by its
    // corner: moved and resized over the cells it is dropped on.
    let anchor = |cx: &mut gpui::VisualTestContext| {
        e.update(cx, |e, _| {
            e.doc.viewer.as_deref_mut().unwrap().charts()[n - 1].anchor
        })
    };
    let before = anchor(cx);
    let body = cx
        .debug_bounds(Box::leak(
            format!("viewer-grid-chart-{}", n - 1).into_boxed_str(),
        ))
        .unwrap();
    let cell = cx
        .debug_bounds(Box::leak(
            format!("viewer-grid-cell-{}-{}", before[0], before[1]).into_boxed_str(),
        ))
        .unwrap();
    let at = body.center();
    let to = at + gpui::point(cell.size.width, cell.size.height * 2.0);
    cx.simulate_mouse_down(at, gpui::MouseButton::Left, gpui::Modifiers::none());
    cx.simulate_mouse_move(to, Some(gpui::MouseButton::Left), gpui::Modifiers::none());
    cx.simulate_mouse_up(to, gpui::MouseButton::Left, gpui::Modifiers::none());
    cx.run_until_parked();
    let moved = anchor(cx);
    assert_eq!(
        moved,
        [before[0] + 2, before[1] + 1, before[2] + 2, before[3] + 1],
        "{before:?} → {moved:?}"
    );
    let corner = cx
        .debug_bounds(Box::leak(
            format!("viewer-grid-chart-corner-{}", n - 1).into_boxed_str(),
        ))
        .unwrap()
        .center();
    let to = corner - gpui::point(gpui::px(0.), cell.size.height * 3.0);
    cx.simulate_mouse_down(corner, gpui::MouseButton::Left, gpui::Modifiers::none());
    cx.simulate_mouse_move(to, Some(gpui::MouseButton::Left), gpui::Modifiers::none());
    cx.simulate_mouse_up(to, gpui::MouseButton::Left, gpui::Modifiers::none());
    cx.run_until_parked();
    let resized = anchor(cx);
    // A pie has no axes: an axis title is refused.
    e.update_in(cx, |e, window, cx| {
        let a = e.doc.viewer.as_deref_mut().unwrap().charts()[n - 1].anchor;
        e.doc
            .viewer
            .as_deref_mut()
            .unwrap()
            .grid_move_to(a[0], a[1]);
        e.run_command(
            "viewer.grid.horizontalAxisTitle",
            serde_json::json!({ "value": "Items" }),
            window,
            cx,
        );
    });
    cx.run_until_parked();
    let titled = e.update(cx, |e, _| {
        e.doc.viewer.as_deref_mut().unwrap().charts()[n - 1]
            .horizontal_title
            .clone()
    });
    assert_eq!(titled, None, "a pie has no axes");
    // Its legend, beneath it as a pie's is, moved to its left.
    e.update_in(cx, |e, window, cx| {
        e.run_command(
            "viewer.grid.chartLegend",
            serde_json::json!({ "position": "left" }),
            window,
            cx,
        );
    });
    cx.run_until_parked();
    let legend = cx
        .debug_bounds(Box::leak(
            format!("viewer-grid-chart-legend-{}", n - 1).into_boxed_str(),
        ))
        .expect("the legend");
    let box_ = cx
        .debug_bounds(Box::leak(
            format!("viewer-grid-chart-{}", n - 1).into_boxed_str(),
        ))
        .unwrap();
    assert!(
        legend.origin.x < box_.center().x && legend.size.height > legend.size.width / 4.0,
        "a column at the left: {legend:?} in {box_:?}"
    );
    // Its slices labeled with their shares, drawn on the plot.
    e.update_in(cx, |e, window, cx| {
        e.run_command(
            "viewer.grid.dataLabels",
            serde_json::json!({ "value": false, "category": true, "series": false, "percent": true, "apply": true }),
            window,
            cx,
        );
    });
    cx.run_until_parked();
    let labels = e.update(cx, |e, _| {
        e.doc.viewer.as_deref_mut().unwrap().charts()[n - 1].labels
    });
    assert!(labels.category && labels.percent && !labels.value);
    // Its first slice dark red: the slices offered, then the color.
    e.update_in(cx, |e, window, cx| {
        e.run_command(
            "viewer.grid.pointColor",
            serde_json::json!({ "point": 0, "color": "#C00000" }),
            window,
            cx,
        );
    });
    cx.run_until_parked();
    let slices = e.update(cx, |e, _| {
        e.doc.viewer.as_deref_mut().unwrap().charts()[n - 1].series[0]
            .point_colors
            .clone()
    });
    assert_eq!(slices, vec![(0, [0xC0, 0, 0])]);
    // That slice pulled out a quarter, then every slice a tenth.
    e.update_in(cx, |e, window, cx| {
        e.run_command(
            "viewer.grid.explodeSlice",
            serde_json::json!({ "point": 0, "percent": 25 }),
            window,
            cx,
        );
    });
    cx.run_until_parked();
    let out = e.update(cx, |e, _| {
        e.doc.viewer.as_deref_mut().unwrap().charts()[n - 1].series[0]
            .point_explosions
            .clone()
    });
    assert_eq!(out, vec![(0, 25)]);
    e.update_in(cx, |e, window, cx| {
        e.run_command(
            "viewer.grid.explodeSlice",
            serde_json::json!({ "point": "all", "value": "10%" }),
            window,
            cx,
        );
    });
    cx.run_until_parked();
    let s0 = e.update(cx, |e, _| {
        e.doc.viewer.as_deref_mut().unwrap().charts()[n - 1].series[0].clone()
    });
    assert_eq!((s0.explosion, s0.point_explosions), (10, vec![]));
    // A column chart on a logarithmic scale, then every 500 from 0.
    e.update_in(cx, |e, window, cx| {
        e.doc.viewer.as_deref_mut().unwrap().grid_move_to(1, 1);
        e.run_command(
            "viewer.grid.insertChart",
            serde_json::json!({ "kind": "column" }),
            window,
            cx,
        );
        let v = e.doc.viewer.as_deref_mut().unwrap();
        let a = v.charts().last().unwrap().anchor;
        v.grid_move_to(a[0], a[1]);
        e.run_command(
            "viewer.grid.axisScale",
            serde_json::json!({ "field": "log" }),
            window,
            cx,
        );
        v_scale_check(e, true);
        e.run_command(
            "viewer.grid.axisScale",
            serde_json::json!({ "field": "log" }),
            window,
            cx,
        );
        e.run_command(
            "viewer.grid.axisScale",
            serde_json::json!({ "field": "major", "value": "500" }),
            window,
            cx,
        );
        e.run_command(
            "viewer.grid.axisScale",
            serde_json::json!({ "field": "min", "value": "0" }),
            window,
            cx,
        );
        e.doc.viewer.as_deref_mut().unwrap().grid_move_to(0, 0);
    });
    cx.run_until_parked();
    let sc = e.update(cx, |e, _| {
        e.doc
            .viewer
            .as_deref_mut()
            .unwrap()
            .charts()
            .last()
            .unwrap()
            .scale
    });
    assert_eq!((sc.min, sc.major, sc.log), (Some(0.0), Some(500.0), false));
    // Made an area chart, its scale kept, and drawn.
    e.update_in(cx, |e, window, cx| {
        let v = e.doc.viewer.as_deref_mut().unwrap();
        let a = v.charts().last().unwrap().anchor;
        v.grid_move_to(a[0], a[1]);
        e.run_command(
            "viewer.grid.chartKind",
            serde_json::json!({ "kind": "area" }),
            window,
            cx,
        );
        e.doc.viewer.as_deref_mut().unwrap().grid_move_to(0, 0);
    });
    cx.run_until_parked();
    let (kind, scale) = e.update(cx, |e, _| {
        let v = e.doc.viewer.as_deref_mut().unwrap();
        let c = v.charts();
        (c.last().unwrap().kind, c.last().unwrap().scale)
    });
    assert_eq!(kind, kalem_viewer::ChartKind::Area);
    assert_eq!(scale, sc);
    // Its first series colored green.
    e.update_in(cx, |e, window, cx| {
        let v = e.doc.viewer.as_deref_mut().unwrap();
        let a = v.charts().last().unwrap().anchor;
        v.grid_move_to(a[0], a[1]);
        e.run_command(
            "viewer.grid.seriesColor",
            serde_json::json!({ "series": 0, "color": "#70AD47" }),
            window,
            cx,
        );
        e.doc.viewer.as_deref_mut().unwrap().grid_move_to(0, 0);
    });
    cx.run_until_parked();
    let color = e.update(cx, |e, _| {
        e.doc
            .viewer
            .as_deref_mut()
            .unwrap()
            .charts()
            .last()
            .unwrap()
            .series[0]
            .color
    });
    assert_eq!(color, Some([0x70, 0xAD, 0x47]));
    // Its area: a light background and no border.
    e.update_in(cx, |e, window, cx| {
        let v = e.doc.viewer.as_deref_mut().unwrap();
        let a = v.charts().last().unwrap().anchor;
        v.grid_move_to(a[0], a[1]);
        e.run_command(
            "viewer.grid.chartArea",
            serde_json::json!({ "part": "background", "color": "#DEEBF7" }),
            window,
            cx,
        );
        e.run_command(
            "viewer.grid.chartArea",
            serde_json::json!({ "part": "border", "color": "none" }),
            window,
            cx,
        );
        e.doc.viewer.as_deref_mut().unwrap().grid_move_to(0, 0);
    });
    cx.run_until_parked();
    let (bg, border) = e.update(cx, |e, _| {
        let c = e.doc.viewer.as_deref_mut().unwrap().charts();
        (c.last().unwrap().background, c.last().unwrap().border)
    });
    assert_eq!(
        (bg, border),
        (
            kalem_viewer::Paint::Color([0xDE, 0xEB, 0xF7]),
            kalem_viewer::Paint::None
        )
    );
    // Its gridlines: minor horizontal ones too, then none horizontal.
    e.update_in(cx, |e, window, cx| {
        let v = e.doc.viewer.as_deref_mut().unwrap();
        let a = v.charts().last().unwrap().anchor;
        v.grid_move_to(a[0], a[1]);
        e.run_command(
            "viewer.grid.gridlines",
            serde_json::json!({ "toggle": "horizontalMinor" }),
            window,
            cx,
        );
        e.run_command(
            "viewer.grid.gridlines",
            serde_json::json!({ "toggle": "horizontalMajor" }),
            window,
            cx,
        );
    });
    cx.simulate_keystrokes("escape");
    cx.run_until_parked();
    let g = e.update(cx, |e, _| {
        e.doc
            .viewer
            .as_deref_mut()
            .unwrap()
            .charts()
            .last()
            .unwrap()
            .gridlines
    });
    assert!(g.horizontal_minor && !g.horizontal_major);
    // Its value axis in thousands.
    e.update_in(cx, |e, window, cx| {
        let v = e.doc.viewer.as_deref_mut().unwrap();
        let a = v.charts().last().unwrap().anchor;
        v.grid_move_to(a[0], a[1]);
        e.run_command(
            "viewer.grid.axisFormat",
            serde_json::json!({ "format": "#,##0,\"K\"" }),
            window,
            cx,
        );
        e.doc.viewer.as_deref_mut().unwrap().grid_move_to(0, 0);
    });
    cx.run_until_parked();
    let f = e.update(cx, |e, _| {
        e.doc
            .viewer
            .as_deref_mut()
            .unwrap()
            .charts()
            .last()
            .unwrap()
            .axis_format
            .clone()
    });
    assert_eq!(f.as_deref(), Some("#,##0,\"K\""));
    // Its value labels bigger and italic.
    e.update_in(cx, |e, window, cx| {
        let v = e.doc.viewer.as_deref_mut().unwrap();
        let a = v.charts().last().unwrap().anchor;
        v.grid_move_to(a[0], a[1]);
        e.run_command(
            "viewer.grid.axisFont",
            serde_json::json!({ "axis": "vertical", "op": "size", "value": "14" }),
            window,
            cx,
        );
        e.run_command(
            "viewer.grid.axisFont",
            serde_json::json!({ "axis": "vertical", "op": "italic" }),
            window,
            cx,
        );
        e.doc.viewer.as_deref_mut().unwrap().grid_move_to(0, 0);
    });
    cx.simulate_keystrokes("escape");
    cx.run_until_parked();
    let vf = e.update(cx, |e, _| {
        e.doc
            .viewer
            .as_deref_mut()
            .unwrap()
            .charts()
            .last()
            .unwrap()
            .vertical_font
            .clone()
    });
    assert_eq!((vf.size, vf.italic), (Some(14.0), true));
    // A title in 20 point Georgia.
    e.update_in(cx, |e, window, cx| {
        let v = e.doc.viewer.as_deref_mut().unwrap();
        let a = v.charts().last().unwrap().anchor;
        v.grid_move_to(a[0], a[1]);
        e.run_command(
            "viewer.grid.chartTitle",
            serde_json::json!({ "value": "Areas" }),
            window,
            cx,
        );
        e.run_command(
            "viewer.grid.titleFont",
            serde_json::json!({ "op": "size", "value": "20" }),
            window,
            cx,
        );
        e.run_command(
            "viewer.grid.titleFont",
            serde_json::json!({ "op": "face", "value": "Georgia" }),
            window,
            cx,
        );
        e.doc.viewer.as_deref_mut().unwrap().grid_move_to(0, 0);
    });
    cx.simulate_keystrokes("escape");
    cx.run_until_parked();
    let (tf, count) = e.update(cx, |e, _| {
        let c = e.doc.viewer.as_deref_mut().unwrap().charts();
        (c.last().unwrap().title_font.clone(), c.len())
    });
    assert_eq!((tf.size, tf.face.as_deref()), (Some(20.0), Some("Georgia")));
    assert!(
        cx.debug_bounds(Box::leak(
            format!("viewer-grid-chart-title-{}", count - 1).into_boxed_str()
        ))
        .is_some()
    );
    // Its plot area a darker blue with a border.
    e.update_in(cx, |e, window, cx| {
        let v = e.doc.viewer.as_deref_mut().unwrap();
        let a = v.charts().last().unwrap().anchor;
        v.grid_move_to(a[0], a[1]);
        e.run_command(
            "viewer.grid.chartArea",
            serde_json::json!({ "part": "plotBackground", "color": "#BDD7EE" }),
            window,
            cx,
        );
        e.run_command(
            "viewer.grid.chartArea",
            serde_json::json!({ "part": "plotBorder", "color": "#264478" }),
            window,
            cx,
        );
        e.doc.viewer.as_deref_mut().unwrap().grid_move_to(0, 0);
    });
    cx.run_until_parked();
    let (pb, pl, count) = e.update(cx, |e, _| {
        let c = e.doc.viewer.as_deref_mut().unwrap().charts();
        (
            c.last().unwrap().plot_background,
            c.last().unwrap().plot_border,
            c.len(),
        )
    });
    assert_eq!(
        (pb, pl),
        (
            kalem_viewer::Paint::Color([0xBD, 0xD7, 0xEE]),
            kalem_viewer::Paint::Color([0x26, 0x44, 0x78])
        )
    );
    assert!(
        cx.debug_bounds(Box::leak(
            format!("viewer-grid-chart-plot-{}", count - 1).into_boxed_str()
        ))
        .is_some()
    );
    assert!(
        cx.debug_bounds(Box::leak(
            format!("viewer-grid-chart-{}", count - 1).into_boxed_str()
        ))
        .is_some()
    );
    assert!(
        cx.debug_bounds(Box::leak(
            format!("viewer-grid-chart-{}", n - 1).into_boxed_str()
        ))
        .is_some()
    );
    assert_eq!(resized[..2], moved[..2]);
    assert_eq!(resized[2], moved[2] - 3, "{moved:?} → {resized:?}");
    let _ = std::fs::remove_dir_all(dir);
}

/// The scale of the last chart, logarithmic or not.
fn v_scale_check(e: &mut kalem_ui::editor::Editor, log: bool) {
    let v = e.doc.viewer.as_deref_mut().unwrap();
    assert_eq!(v.charts().last().unwrap().scale.log, log);
}

/// E44: a pattern fill and slanted text drawn, a gradient and a double
/// border kept; Format Cells listing every part.
#[gpui::test]
fn formatting_the_rest(cx: &mut TestAppContext) {
    let (ws, dir, cx) = open(cx);
    let e = ws.read_with(cx, |ws, _| ws.editor.clone());
    e.update_in(cx, |e, window, cx| {
        let v = e.doc.viewer.as_deref_mut().unwrap();
        v.grid_move_to(1, 1);
        e.run_command(
            "viewer.grid.fillEffect",
            serde_json::json!({ "effect": "darkUp" }),
            window,
            cx,
        );
        let v = e.doc.viewer.as_deref_mut().unwrap();
        v.grid_move_to(1, 0);
        v.change_style(kalem_viewer::StyleChange {
            rotation: Some(45),
            ..Default::default()
        })
        .unwrap();
        v.grid_move_to(2, 1);
        e.run_command(
            "viewer.grid.fillEffect",
            serde_json::json!({ "effect": "gradient0" }),
            window,
            cx,
        );
        e.run_command(
            "viewer.grid.borderLine",
            serde_json::json!({ "line": "double", "set": "outside" }),
            window,
            cx,
        );
        e.run_command("viewer.grid.formatCells", serde_json::json!({}), window, cx);
    });
    cx.run_until_parked();
    assert!(cx.debug_bounds("viewer-grid-pattern-1-1").is_some());
    assert!(cx.debug_bounds("viewer-grid-rotated-1-0").is_some());
    let (pattern, line) = e.update(cx, |e, _| {
        let v = e.doc.viewer.as_deref_mut().unwrap();
        let c = v.cursor_cell();
        (c.fill_pattern, c.border_styles[0])
    });
    assert!(matches!(
        pattern,
        Some(kalem_viewer::FillPattern::Gradient { angle: 0, .. })
    ));
    assert_eq!(line, Some(kalem_viewer::LineStyle::Double));
    let _ = std::fs::remove_dir_all(dir);
}

/// Typing on a cell starts its entry with the character, and the entry
/// takes the keys typed after it (a viewer draws no text to take them).
#[gpui::test]
fn typing_on_a_cell(cx: &mut TestAppContext) {
    let (ws, dir, cx) = open(cx);
    let e = ws.read_with(cx, |ws, _| ws.editor.clone());
    e.update(cx, |e, _| {
        e.doc.viewer.as_deref_mut().unwrap().grid_move_to(10, 1)
    });
    let key = |s: &str| gpui::KeyDownEvent {
        keystroke: gpui::Keystroke {
            key: s.into(),
            key_char: (s.chars().count() == 1).then(|| s.into()),
            ..Default::default()
        },
        is_held: false,
        prefer_character_input: false,
    };
    for k in ["4", "2"] {
        e.update_in(cx, |e, window, cx| e.key_down(&key(k), window, cx));
        cx.run_until_parked();
    }
    // Shown in the cell as typed.
    assert!(cx.debug_bounds("viewer-grid-entry").is_some());
    e.update_in(cx, |e, window, cx| e.key_down(&key("enter"), window, cx));
    cx.run_until_parked();
    assert!(cx.debug_bounds("viewer-grid-entry").is_none());
    let t = e.update(cx, |e, _| {
        let v = e.doc.viewer.as_deref_mut().unwrap();
        v.grid_move_to(10, 1);
        v.cell_input()
    });
    assert_eq!(t, "42");
    let _ = std::fs::remove_dir_all(dir);
}

/// E46: a combo chart with a trendline and error bars, and the kinds
/// drawn from shapes, painted without trouble.
#[gpui::test]
fn charts_the_rest(cx: &mut TestAppContext) {
    let (ws, dir, cx) = open(cx);
    let e = ws.read_with(cx, |ws, _| ws.editor.clone());
    let run = |cx: &mut VisualTestContext, id: &str, args: serde_json::Value| {
        e.update_in(cx, |e, window, cx| e.run_command(id, args, window, cx));
        cx.run_until_parked();
    };
    e.update(cx, |e, _| {
        e.doc.viewer.as_deref_mut().unwrap().grid_move_to(1, 1)
    });
    run(
        cx,
        "viewer.grid.insertChart",
        serde_json::json!({ "kind": "column", "title": "Spending" }),
    );
    let (n, a) = e.update(cx, |e, _| {
        let v = e.doc.viewer.as_deref_mut().unwrap();
        let c = v.charts();
        let a = c.last().unwrap().anchor;
        v.grid_move_to(a[0] + 1, a[1] + 1);
        (v.chart_at_cursor().unwrap().0, a)
    });
    run(
        cx,
        "viewer.grid.seriesKind",
        serde_json::json!({ "series": 1, "kind": "line", "secondary": true }),
    );
    run(
        cx,
        "viewer.grid.trendline",
        serde_json::json!({ "series": 0, "kind": "linear", "show": "both" }),
    );
    run(
        cx,
        "viewer.grid.errorBars",
        serde_json::json!({ "series": 0, "kind": "stdErr" }),
    );
    e.update(cx, |e, _| {
        e.doc.viewer.as_deref_mut().unwrap().grid_move_to(a[0], 0)
    });
    cx.run_until_parked();
    let key = format!("viewer-grid-chart-plot-{n}");
    assert!(
        cx.debug_bounds(Box::leak(key.clone().into_boxed_str()))
            .is_some()
    );
    for kind in ["radar", "bubble", "stock"] {
        e.update(cx, |e, _| {
            e.doc
                .viewer
                .as_deref_mut()
                .unwrap()
                .grid_move_to(a[0] + 1, a[1] + 1)
        });
        run(
            cx,
            "viewer.grid.chartKind",
            serde_json::json!({ "kind": kind }),
        );
        e.update(cx, |e, _| {
            e.doc.viewer.as_deref_mut().unwrap().grid_move_to(a[0], 0)
        });
        cx.run_until_parked();
        let k = e.update(cx, |e, _| {
            e.doc.viewer.as_deref_mut().unwrap().charts()[n].kind
        });
        assert_eq!(format!("{k:?}").to_lowercase(), kind);
        assert!(
            cx.debug_bounds(Box::leak(key.clone().into_boxed_str()))
                .is_some()
        );
    }
    let _ = std::fs::remove_dir_all(dir);
}

/// E47: a pivot table's slicer drawn, a click on an item choosing it alone.
#[gpui::test]
fn pivot_slicer_clicked(cx: &mut TestAppContext) {
    let (ws, dir, cx) = open(cx);
    let e = ws.read_with(cx, |ws, _| ws.editor.clone());
    e.update(cx, |e, cx| {
        let v = e.doc.viewer.as_deref_mut().unwrap();
        v.insert_pivot(kalem_viewer::PivotSpec {
            range: [0, 0, 4, 2],
            rows: vec![0],
            values: vec![kalem_viewer::PivotSpec::value(
                1,
                kalem_viewer::Aggregate::Sum,
            )],
            ..kalem_viewer::PivotSpec::default()
        })
        .unwrap();
        v.grid_move_to(3, 0);
        v.insert_slicer("Item").unwrap();
        v.grid_move_to(0, 0);
        cx.notify();
    });
    cx.run_until_parked();
    assert!(cx.debug_bounds("viewer-grid-slicer-0").is_some());
    let second = cx
        .debug_bounds("viewer-grid-slicer-0-1")
        .expect("its second item");
    cx.simulate_click(second.center(), gpui::Modifiers::default());
    cx.run_until_parked();
    let on = e.update(cx, |e, _| {
        e.doc.viewer.as_deref_mut().unwrap().slicers()[0]
            .items
            .iter()
            .filter(|i| i.1)
            .count()
    });
    assert_eq!(on, 1);
    let _ = std::fs::remove_dir_all(dir);
}

/// Saving an OpenDocument spreadsheet, which opened converted to a
/// workbook, writes a new file of what the conversion kept: the editor
/// asks first (Cancel leaves the file as it was), once a document.
#[gpui::test]
fn a_converted_file_asks_before_it_is_saved(cx: &mut TestAppContext) {
    let (ws, dir, cx) = open(cx);
    let data =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../kalem-tui/tests/data/budget.ods");
    let ods = dir.join("lo.ods");
    std::fs::copy(data, &ods).unwrap();
    ws.update_in(cx, |ws, window, cx| ws.open(&ods, None, window, cx));
    cx.run_until_parked();
    let e = ws.read_with(cx, |ws, _| ws.editor.clone());
    let set = |value: &str, cx: &mut VisualTestContext| {
        e.update(cx, |e, _| {
            e.doc
                .viewer
                .as_deref_mut()
                .unwrap()
                .set_cell(1, 1, value)
                .unwrap();
        });
    };
    let save = |cx: &mut VisualTestContext| {
        e.update_in(cx, |e, window, cx| {
            e.run_command("app.save", serde_json::Value::Null, window, cx);
        });
        cx.run_until_parked();
    };
    let modified = |cx: &mut VisualTestContext| e.read_with(cx, |e, _| e.doc.is_modified());
    set("1300", cx);
    let before = std::fs::read(&ods).unwrap();
    save(cx);
    assert!(cx.has_pending_prompt());
    cx.simulate_prompt_answer("Cancel");
    cx.run_until_parked();
    assert!(modified(cx));
    assert_eq!(std::fs::read(&ods).unwrap(), before);
    save(cx);
    cx.simulate_prompt_answer("Save as .ods");
    cx.run_until_parked();
    assert!(!modified(cx));
    assert_ne!(std::fs::read(&ods).unwrap(), before);
    set("1400", cx);
    save(cx);
    assert!(!cx.has_pending_prompt());
    assert!(!modified(cx));
}

/// A workbook's menus and toolbar carry its commands where Excel has
/// them, not a text's or a picture's; Bold on the toolbar bolds the cell.
#[gpui::test]
fn a_workbooks_menus_and_toolbar(cx: &mut TestAppContext) {
    let (ws, dir, cx) = open(cx);
    let e = ws.read_with(cx, |ws, _| ws.editor.clone());
    let doc = e.read_with(cx, |e, _| e.doc.document_context());
    let registry = kalem_core::CommandRegistry::with_builtins();
    let menus = kalem_ui::workspace::menus_for(&registry, &doc);
    let names: Vec<String> = menus.iter().map(|m| m.name.to_string()).collect();
    for n in [
        "File", "Edit", "Format", "Insert", "Data", "Formulas", "Chart", "View",
    ] {
        assert!(names.iter().any(|m| m == n), "{n}: {names:?}");
    }
    // CSV's Table and BibTeX's menus are not a workbook's.
    assert!(
        !names.iter().any(|m| m == "Table" || m == "BibTeX"),
        "{names:?}"
    );
    // The grid's Bold button, not Org's: it turns the header's bold off.
    assert!(cx.debug_bounds("tool-0").is_none(), "no Org Bold button");
    let bold = |cx: &mut VisualTestContext| {
        e.update(cx, |e, _| {
            e.doc.viewer.as_deref_mut().unwrap().cursor_cell()
        })
        .bold
    };
    assert!(bold(cx), "A1 is bold");
    let button = cx.debug_bounds("tool-24").expect("the workbook's Bold");
    cx.simulate_click(button.center(), gpui::Modifiers::default());
    cx.run_until_parked();
    assert!(!bold(cx));
    let _ = std::fs::remove_dir_all(&dir);
}
