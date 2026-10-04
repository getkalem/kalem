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
