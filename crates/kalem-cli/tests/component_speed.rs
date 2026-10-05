//! wasm_todo W6: the bundled plugins, components released by
//! getkalem/plugins, measured in Kalem, each measurement with its ceiling:
//! a workbook of 36,000 cells (opened, scrolled, a held arrow key, typing,
//! a 100,000-cell paste, sorting), one of a million (opened, seen, typed
//! in, and the memory it needs), a 6-megapixel picture and a long PDF.
//! The ceilings are about ten times a debug build's times on an M1 Max,
//! for slower machines; the measurements against the native copies,
//! which Kalem no longer has, are in `docs/wasm_todo.md` (W6). With the
//! numbers:
//!
//! ```sh
//! cargo test -p kalem-cli --test component_speed -- --nocapture
//! ```
//!
//! `KALEM_SPEED_PDF` names another PDF than the one made here.
#![allow(clippy::print_stdout)]

mod common;

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use kalem_core::viewer::ViewerState;
use kalem_viewer::{FileHandle, Viewer};

/// The terminal editor's budget with `rows` from row 10, saved as `name`
/// in `dir`, made through the workbook component.
fn workbook(dir: &Path, name: &str, rows: &[Vec<String>]) -> PathBuf {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("../kalem-tui/tests/data/budget.xlsx");
    let mut wb = common::viewer("org.kalem.xlsx")
        .open(FileHandle::new(&src))
        .unwrap();
    wb.set_cells(0, 9, 0, rows).unwrap();
    let path = dir.join(name);
    std::fs::write(&path, wb.save().unwrap().bytes).unwrap();
    path
}

/// A workbook of 3,000 rows of twelve columns, a SUM in the last.
fn big_workbook(dir: &Path) -> PathBuf {
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
    workbook(dir, "big.xlsx", &rows)
}

/// What the window asks of the grid for a frame of `rows` × `cols` from
/// row `top`, nothing changed since the last.
fn frame(v: &mut ViewerState, top: u32, rows: u32, cols: u32) {
    let _ = v.grid_layout();
    let _ = v.sheet_tabs();
    let _ = v.panes();
    let _ = v.page_breaks();
    let _ = v.grid_zoom();
    v.set_grid_visible(rows, cols);
    let _ = v.grid_cells(top..top + rows, 0..cols);
    let _ = v.invalid_cells(top..top + rows, 0..cols);
    let _ = v.cell_input();
    let _ = v.cursor_has_list();
    let _ = v.selection_name();
    let _ = v.outline_marks();
    let _ = v.drawings();
}

fn time<R>(f: impl FnOnce() -> R) -> Duration {
    let t = Instant::now();
    let _ = f();
    t.elapsed()
}

/// The measurements through `viewer`.
fn measure(viewer: Arc<dyn Viewer>, path: &Path) -> Vec<(&'static str, Duration)> {
    let mut out = Vec::new();
    let mut state = None;
    out.push((
        "open",
        time(|| state = Some(ViewerState::open(viewer, path).unwrap())),
    ));
    let mut v = state.unwrap();
    out.push((
        "layout x100",
        time(|| {
            for _ in 0..100 {
                let _ = v.grid_layout();
            }
        }),
    ));
    out.push(("cells 50x12 fresh", time(|| v.grid_cells(400..450, 0..12))));
    out.push(("cells 50x12 again", time(|| v.grid_cells(400..450, 0..12))));
    v.grid_move_to(400, 0);
    out.push((
        "60 frames unchanged",
        time(|| {
            for _ in 0..60 {
                frame(&mut v, 400, 50, 12);
            }
        }),
    ));
    // A held arrow key: a step, then what a frame asks.
    out.push((
        "200 steps down",
        time(|| {
            for r in 0..200 {
                v.grid_move_to(500 + r, 2);
                let _ = v.grid_layout();
                let _ = v.grid_cells(480 + r..530 + r, 0..12);
            }
        }),
    ));
    out.push(("one cell typed", time(|| v.set_cell(20, 1, "123"))));
    let tsv: String = (0..1000)
        .map(|r| {
            (0..100)
                .map(|c| ((r * 3 + c) % 97).to_string())
                .collect::<Vec<_>>()
                .join("\t")
        })
        .collect::<Vec<_>>()
        .join("\n");
    v.grid_move_to(4000, 0);
    out.push(("paste 100,000 cells", time(|| v.paste_text(&tsv))));
    v.grid_move_to(10, 1);
    out.push(("sort 3,000 rows", time(|| v.sort(true))));
    out
}

/// A workbook of a million cells: 100,000 rows of ten numbers.
fn million_cells(dir: &Path) -> PathBuf {
    let rows: Vec<Vec<String>> = (0..100_000)
        .map(|r| {
            (0..10)
                .map(|c| ((r * 31 + c * 7) % 10_000).to_string())
                .collect()
        })
        .collect();
    workbook(dir, "million.xlsx", &rows)
}

/// The million-cell workbook through `viewer`: opened, seen, a cell typed.
fn measure_million(viewer: Arc<dyn Viewer>, path: &Path) -> Vec<(&'static str, Duration)> {
    let mut out = Vec::new();
    let mut state = None;
    out.push((
        "1M: open",
        time(|| state = Some(ViewerState::open(viewer, path).unwrap())),
    ));
    let mut v = state.unwrap();
    out.push((
        "1M: cells 50x10",
        time(|| v.grid_cells(50_000..50_050, 0..10)),
    ));
    v.grid_move_to(50_000, 0);
    out.push((
        "1M: 60 frames",
        time(|| {
            for _ in 0..60 {
                frame(&mut v, 50_000, 50, 10);
            }
        }),
    ));
    out.push((
        "1M: one cell typed",
        time(|| v.set_cell(50_010, 3, "=SUM(A50011:C50011)")),
    ));
    out
}

/// A PDF's first fifty pages rendered through `viewer`, at the scale a
/// page is read at.
fn measure_pdf(viewer: Arc<dyn Viewer>, path: &Path) -> Vec<(&'static str, Duration)> {
    let mut out = Vec::new();
    let mut state = None;
    out.push((
        "pdf: open",
        time(|| state = Some(ViewerState::open(viewer, path).unwrap())),
    ));
    let mut v = state.unwrap();
    v.set_area(900.0, 1200.0);
    let n = v.structure().units.len().min(50);
    out.push((
        "pdf: 50 pages drawn",
        time(|| {
            for u in 0..n {
                v.go_to(u);
                let _ = v.bitmap();
            }
        }),
    ));
    out
}

/// A picture opened and drawn, then drawn again.
fn measure_picture(viewer: Arc<dyn Viewer>, path: &Path) -> Vec<(&'static str, Duration)> {
    let mut out = Vec::new();
    let mut state = None;
    out.push((
        "picture: open, drawn",
        time(|| {
            let mut v = ViewerState::open(viewer, path).unwrap();
            v.set_area(1200.0, 800.0);
            let _ = v.bitmap();
            state = Some(v);
        }),
    ));
    let mut v = state.unwrap();
    out.push(("picture: drawn again", time(|| v.bitmap())));
    out
}

/// The measurements against their ceilings, in milliseconds.
fn check(measured: &[(&str, Duration)], ceilings: &[u128]) {
    assert_eq!(measured.len(), ceilings.len());
    for ((label, t), ceiling) in measured.iter().zip(ceilings) {
        println!(
            "{label:<22} {:>9.1} ms  (ceiling {ceiling} ms)",
            t.as_secs_f64() * 1000.0
        );
        assert!(
            t.as_millis() <= *ceiling,
            "{label}: {} ms, past its ceiling of {ceiling} ms",
            t.as_millis()
        );
    }
}

/// The smallest memory limit, of a few, under which the component opens
/// `path` and shows its cells.
fn least_memory(id: &str, path: &Path) -> Option<usize> {
    let c = common::built_in(id);
    let host = Arc::new(kalem_script::Host::new(None).unwrap());
    [64, 128, 256, 384, 512, 768, 1024, 2048, 4096]
        .into_iter()
        .find(|mb| {
            let v = kalem_script::viewer::ComponentViewer::embedded(
                host.clone(),
                move || c.wasm(),
                "xlsx",
                "Excel workbooks",
                &["xlsx".to_string()],
                kalem_script::Limits {
                    memory: mb << 20,
                    ..kalem_script::viewer::VIEWER_LIMITS
                },
            );
            ViewerState::open(Arc::new(v), path)
                .is_ok_and(|mut s| !s.grid_cells(0..50, 0..10).is_empty())
        })
}

/// The built-in component `id`, compiled, saying how long that took.
fn component(id: &str) -> Arc<dyn Viewer> {
    let v = common::viewer(id);
    let t = Instant::now();
    v.plugin().unwrap();
    println!("{id}: component compiled in {} ms", t.elapsed().as_millis());
    v
}

#[test]
fn components_measured_against_their_ceilings() {
    let dir = common::scratch("component-speed");
    let path = big_workbook(&dir);
    let xlsx = component("org.kalem.xlsx");
    // open, layout ×100, cells fresh, cells again, 60 frames, 200 steps,
    // a cell typed, a paste, a sort.
    check(
        &measure(xlsx.clone(), &path),
        &[500, 100, 300, 100, 1500, 4000, 1000, 5000, 5000],
    );
    let million = million_cells(&dir);
    // open, cells, 60 frames, a cell typed.
    check(
        &measure_million(xlsx, &million),
        &[10_000, 2000, 4000, 8000],
    );
    // A million cells fit in half of a viewer's default memory.
    let least = least_memory("org.kalem.xlsx", &million);
    println!("1M: least memory       {least:?} MB");
    assert!(least.is_some_and(|mb| mb <= 512), "{least:?}");
    let pic = common::big_picture(&dir);
    check(
        &measure_picture(component("org.kalem.image-viewer"), &pic),
        &[2000, 100],
    );
    let pdf = std::env::var_os("KALEM_SPEED_PDF")
        .map_or_else(|| common::long_pdf(&dir, 60), PathBuf::from);
    check(
        &measure_pdf(component("org.kalem.pdf-viewer"), &pdf),
        &[500, 20_000],
    );
    let _ = std::fs::remove_dir_all(&dir);
}
