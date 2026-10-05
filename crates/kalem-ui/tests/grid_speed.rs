//! How long a cursor step takes in a large workbook in the graphical
//! editor, drawn.
#![allow(clippy::print_stdout)]

use std::rc::Rc;
use std::time::Instant;

use gpui::TestAppContext;
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

#[gpui::test]
#[ignore = "a measurement: cargo test --release -p kalem-ui --test grid_speed -- --ignored --nocapture"]
fn holding_down(cx: &mut TestAppContext) {
    kalem_core::viewer::register(kalem_components::viewer("org.kalem.xlsx").unwrap());
    let dir = std::env::temp_dir().join(format!("kalem-ui-grid-speed-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let src = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../kalem-tui/tests/data/budget.xlsx");
    // Made through the workbook component, as the editor opens it.
    let mut wb = kalem_viewer::Viewer::open(
        &*kalem_components::viewer("org.kalem.xlsx").unwrap(),
        kalem_viewer::FileHandle::new(&src),
    )
    .unwrap();
    let rows: Vec<Vec<String>> = (0..3000)
        .map(|r| {
            (0..12)
                .map(|c| match c {
                    0 => format!("Item {r}"),
                    11 => format!("=SUM(B{}:K{})", r + 10, r + 10),
                    _ => format!("{}", (r * 7 + c * 13) % 1000),
                })
                .collect()
        })
        .collect();
    wb.set_cells(0, 9, 0, &rows).unwrap();
    let path = dir.join("big.xlsx");
    std::fs::write(&path, wb.save().unwrap().bytes).unwrap();
    std::fs::write(dir.join("notes.org"), "* Notes\n").unwrap();
    let shared = Rc::new(kalem_ui::shared_in(Config::default(), test_settings(&dir)));
    let notes = dir.join("notes.org");
    let (ws, cx) = cx.add_window_view(|window, cx| {
        let e = kalem_ui::editor::open(Some(&notes), shared, Theme::light(), cx).unwrap();
        window.focus(&gpui::Focusable::focus_handle(e.read(cx), cx), cx);
        Workspace::new(e, window, cx)
    });
    cx.run_until_parked();
    ws.update_in(cx, |ws, window, cx| ws.open(&path, None, window, cx));
    cx.run_until_parked();
    for key in ["down", "right", "up"] {
        let n = 200;
        let start = Instant::now();
        let mut worst = 0u128;
        for _ in 0..n {
            let t = Instant::now();
            cx.simulate_keystrokes(key);
            cx.run_until_parked();
            worst = worst.max(t.elapsed().as_micros());
        }
        println!(
            "{key}: {} µs a step, worst {worst} µs",
            start.elapsed().as_micros() / n
        );
    }
    let _ = std::fs::remove_dir_all(&dir);
}
