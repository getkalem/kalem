//! How long a cursor step takes in a large workbook, drawn.
#![allow(clippy::print_stdout)]

use std::sync::Arc;
use std::time::Instant;

use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
use kalem_core::settings::Config;
use kalem_tui::app::App;
use kalem_tui::caps::Caps;
use ratatui::Terminal;
use ratatui::backend::TestBackend;

#[test]
#[ignore = "a measurement: cargo test --release -p kalem-tui --test grid_speed -- --ignored --nocapture"]
fn holding_down() {
    kalem_core::viewer::register(Arc::new(kalem_plugin_xlsx::XlsxViewer));
    let dir = std::env::temp_dir().join(format!("kalem-grid-speed-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let src = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/budget.xlsx");
    let mut wb = kalem_plugin_xlsx::Workbook::open(std::fs::read(src).unwrap()).unwrap();
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
    wb.set_cells(0, kalem_plugin_xlsx::CellRef::new(9, 0), &rows)
        .unwrap();
    let path = dir.join("big.xlsx");
    std::fs::write(&path, wb.save().unwrap()).unwrap();
    let mut app = App::with_keymap(
        Some(&path),
        Config::default(),
        Caps::full(),
        &[],
        Vec::new(),
    )
    .unwrap();
    {
        let v = app.doc.viewer.as_deref_mut().unwrap();
        for (label, r0) in [
            ("fresh rows", 400u32),
            ("same rows", 400u32),
            ("next fresh", 1000u32),
        ] {
            let t = Instant::now();
            let _ = v.grid_cells(r0..r0 + 50, 0..12);
            println!("grid_cells {label}: {} µs", t.elapsed().as_micros());
        }
        let t = Instant::now();
        for _ in 0..100 {
            let _ = v.grid_layout();
        }
        println!("grid_layout x100: {} µs", t.elapsed().as_micros());
        let t = Instant::now();
        let _ = v.status();
        println!("status: {} µs", t.elapsed().as_micros());
        let t = Instant::now();
        let _ = v.charts();
        println!("charts: {} µs", t.elapsed().as_micros());
        for r in [2000u32, 2001, 2002] {
            let t = Instant::now();
            v.grid_move_to(r, 0);
            println!("move to {r}: {} µs", t.elapsed().as_micros());
        }
        v.grid_move_to(0, 0);
    }
    let mut term = Terminal::new(TestBackend::new(160, 50)).unwrap();
    term.draw(|f| app.draw(f)).unwrap();
    for (name, code) in [
        ("down", KeyCode::Down),
        ("right", KeyCode::Right),
        ("up", KeyCode::Up),
    ] {
        let start = Instant::now();
        let n = 300;
        let mut worst = 0u128;
        for _ in 0..n {
            let t = Instant::now();
            app.event(Event::Key(KeyEvent::new(code, KeyModifiers::NONE)));
            term.draw(|f| app.draw(f)).unwrap();
            worst = worst.max(t.elapsed().as_micros());
        }
        let per = start.elapsed().as_micros() / n;
        println!("{name}: {per} µs a step, worst {worst} µs");
    }
    let _ = std::fs::remove_dir_all(&dir);
}
