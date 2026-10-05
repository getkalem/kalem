//! The user's own lists a fill goes round, made from cells or typed,
//! kept in the settings (a settings folder of the test's own).

use kalem_core::settings::Config;
use kalem_tui::app::App;
use kalem_tui::caps::Caps;
use serde_json::json;

#[test]
fn custom_lists_fill() {
    let dir = std::env::temp_dir().join(format!("kalem-custom-lists-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("config")).unwrap();
    kalem_core::viewer::register(kalem_components::viewer("org.kalem.xlsx").unwrap());
    let src = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/budget.xlsx");
    std::fs::copy(src, dir.join("budget.xlsx")).unwrap();
    let mut app = App::with_keymap(
        Some(&dir.join("budget.xlsx")),
        Config::default(),
        Caps::full(),
        &[],
        Vec::new(),
    )
    .unwrap();
    app.config_dir = Some(dir.join("config"));
    let set = |app: &mut App, row: u32, col: u32, value: &str| {
        app.run_command(
            "viewer.grid.setCell",
            json!({ "row": row, "col": col, "value": value }),
        );
    };
    let input = |app: &mut App, row: u32, col: u32| {
        let v = app.doc.viewer.as_deref_mut().unwrap();
        v.grid_move_to(row, col);
        v.cell_input()
    };
    // Low, Mid, High in H2:H4, selected and added as a list.
    for (k, w) in ["Low", "Mid", "High"].iter().enumerate() {
        set(&mut app, 1 + k as u32, 7, w);
    }
    {
        let v = app.doc.viewer.as_deref_mut().unwrap();
        v.grid_move_to(1, 7);
        v.grid_extend_to(3, 7);
    }
    app.run_command("viewer.grid.customLists", json!({ "op": "selection" }));
    assert_eq!(
        app.config.strings("spreadsheet.custom_lists"),
        ["Low, Mid, High"]
    );
    // A typed one too, then the first removed.
    app.run_command(
        "viewer.grid.customLists",
        json!({ "op": "typed", "value": "Kuzey, Güney, Doğu, Batı" }),
    );
    assert_eq!(app.config.strings("spreadsheet.custom_lists").len(), 2);
    // A fill goes round them: Mid → High, Low; Doğu → Batı, Kuzey.
    set(&mut app, 7, 0, "Mid");
    app.run_command(
        "viewer.grid.fillSeries",
        json!({ "source": [7, 0, 7, 0], "target": [7, 0, 9, 0] }),
    );
    assert_eq!(
        (input(&mut app, 8, 0), input(&mut app, 9, 0)),
        ("High".into(), "Low".into())
    );
    set(&mut app, 7, 1, "Doğu");
    app.run_command(
        "viewer.grid.fillSeries",
        json!({ "source": [7, 1, 7, 1], "target": [7, 1, 9, 1] }),
    );
    assert_eq!(input(&mut app, 9, 1), "Kuzey");
    app.run_command(
        "viewer.grid.customLists",
        json!({ "op": "remove", "index": 0 }),
    );
    assert_eq!(
        app.config.strings("spreadsheet.custom_lists"),
        ["Kuzey, Güney, Doğu, Batı"]
    );
    // Without its list, Mid is only copied.
    set(&mut app, 12, 0, "Mid");
    app.run_command(
        "viewer.grid.fillSeries",
        json!({ "source": [12, 0, 12, 0], "target": [12, 0, 13, 0] }),
    );
    assert_eq!(input(&mut app, 13, 0), "Mid");
    let _ = std::fs::remove_dir_all(&dir);
}
