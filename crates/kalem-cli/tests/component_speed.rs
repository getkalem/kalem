//! wasm_todo W6: the measurements of the bundled viewers' native copies
//! repeated through the components built into Kalem, side by side, each
//! with its ceiling: a workbook of 36,000 cells (opened, scrolled, a held
//! arrow key, typing, a 100,000-cell paste, sorting), one of a million
//! (opened, seen, typed in, and the memory it needs), a 6-megapixel
//! picture and a long PDF. With the numbers:
//!
//! ```sh
//! cargo test -p kalem-cli --features components --test component_speed -- --nocapture
//! ```
//!
//! `KALEM_SPEED_PDF` names another PDF than the one made here.
#![allow(clippy::print_stdout)]

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use kalem_core::viewer::ViewerState;
use kalem_viewer::Viewer;

/// A workbook of 3,000 rows of twelve columns, a SUM in the last.
fn big_workbook(dir: &Path) -> PathBuf {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("../kalem-tui/tests/data/budget.xlsx");
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
    path
}

/// A picture of 3,000 × 2,000 pixels, in shades.
fn big_picture(dir: &Path) -> PathBuf {
    let img = image::RgbImage::from_fn(3000, 2000, |x, y| {
        image::Rgb([(x % 256) as u8, (y % 256) as u8, ((x + y) % 256) as u8])
    });
    let path = dir.join("big.png");
    img.save(&path).unwrap();
    path
}

/// A PDF of sixty pages of text in a standard font.
fn long_pdf(dir: &Path) -> PathBuf {
    let pages = 60;
    let mut objects: Vec<String> = vec![
        "<< /Type /Catalog /Pages 2 0 R >>".into(),
        String::new(),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".into(),
    ];
    let mut kids = Vec::new();
    for p in 0..pages {
        let mut text = String::from("BT /F1 10 Tf 12 TL 56 780 Td\n");
        for line in 0..60 {
            text += &format!(
                "(Page {} line {}: the quick brown fox jumps over the lazy dog, again and again.) '\n",
                p + 1,
                line + 1
            );
        }
        text += "ET";
        let content = objects.len() + 1;
        objects.push(format!(
            "<< /Length {} >>\nstream\n{text}\nendstream",
            text.len()
        ));
        kids.push(format!("{} 0 R", objects.len() + 1));
        objects.push(format!(
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 595 842] \
             /Resources << /Font << /F1 3 0 R >> >> /Contents {content} 0 R >>"
        ));
    }
    objects[1] = format!(
        "<< /Type /Pages /Kids [{}] /Count {pages} >>",
        kids.join(" ")
    );
    let mut pdf = String::from("%PDF-1.4\n");
    let mut offsets = Vec::new();
    for (i, o) in objects.iter().enumerate() {
        offsets.push(pdf.len());
        pdf += &format!("{} 0 obj\n{o}\nendobj\n", i + 1);
    }
    let xref = pdf.len();
    pdf += &format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1);
    for o in offsets {
        pdf += &format!("{o:010} 00000 n \n");
    }
    pdf += &format!(
        "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
        objects.len() + 1
    );
    let path = dir.join("long.pdf");
    std::fs::write(&path, pdf).unwrap();
    path
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
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("../kalem-tui/tests/data/budget.xlsx");
    let mut wb = kalem_plugin_xlsx::Workbook::open(std::fs::read(src).unwrap()).unwrap();
    let rows: Vec<Vec<String>> = (0..100_000)
        .map(|r| {
            (0..10)
                .map(|c| ((r * 31 + c * 7) % 10_000).to_string())
                .collect()
        })
        .collect();
    wb.set_cells(0, kalem_plugin_xlsx::CellRef::new(9, 0), &rows)
        .unwrap();
    let path = dir.join("million.xlsx");
    std::fs::write(&path, wb.save().unwrap()).unwrap();
    path
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

/// The two columns side by side. The ceiling: a component more than four
/// times slower than the native copy fails, unless it is still done
/// within a frame (16 ms); a ratio, as the times are the machine's.
fn table(native: &[(&str, Duration)], through: &[(&str, Duration)]) {
    for ((label, a), (_, b)) in native.iter().zip(through) {
        let ratio = b.as_secs_f64() / a.as_secs_f64().max(1e-9);
        println!(
            "{label:<22} {:>9.1} ms {:>9.1} ms {:>6.1}x",
            a.as_secs_f64() * 1000.0,
            b.as_secs_f64() * 1000.0,
            ratio
        );
        assert!(
            ratio < 4.0 || b.as_millis() < 16,
            "{label}: the component takes {ratio:.1} times the native copy's time"
        );
    }
}

/// The smallest memory limit, of a few, under which the component opens
/// `path` and shows its cells.
fn least_memory(id: &str, path: &Path) -> Option<usize> {
    let c = kalem_components::components()
        .iter()
        .find(|c| c.id == id)
        .expect("the component");
    let host = Arc::new(kalem_script::Host::new(None).unwrap());
    [64, 128, 256, 384, 512, 768, 1024, 2048, 4096]
        .into_iter()
        .find(|mb| {
            let v = kalem_script::viewer::ComponentViewer::embedded(
                host.clone(),
                c.bytes,
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

fn component(id: &str, name: &str, ext: &[&str]) -> Arc<dyn Viewer> {
    let c = kalem_components::components()
        .iter()
        .find(|c| c.id == id)
        .expect("the component");
    let host = Arc::new(kalem_script::Host::new(None).unwrap());
    let v = kalem_script::viewer::ComponentViewer::embedded(
        host,
        c.bytes,
        id.rsplit('.').next().unwrap_or(id),
        name,
        &ext.iter().map(|e| (*e).to_string()).collect::<Vec<_>>(),
        kalem_script::viewer::VIEWER_LIMITS,
    );
    let t = Instant::now();
    v.plugin().unwrap();
    println!(
        "{name}: component compiled in {} ms",
        t.elapsed().as_millis()
    );
    Arc::new(v)
}

#[test]
fn components_keep_up_with_native_copies() {
    let dir = std::env::temp_dir().join(format!("kalem-component-speed-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let path = big_workbook(&dir);
    let native = measure(Arc::new(kalem_plugin_xlsx::XlsxViewer), &path);
    let xlsx = component("org.kalem.xlsx", "Excel workbooks", &["xlsx"]);
    let through = measure(xlsx.clone(), &path);
    println!(
        "{:<22} {:>12} {:>12} {:>7}",
        "", "native", "component", "ratio"
    );
    table(&native, &through);
    let million = million_cells(&dir);
    table(
        &measure_million(Arc::new(kalem_plugin_xlsx::XlsxViewer), &million),
        &measure_million(xlsx, &million),
    );
    // A million cells fit in half of a viewer's default memory.
    let least = least_memory("org.kalem.xlsx", &million);
    println!("1M: least memory       {least:?} MB");
    assert!(least.is_some_and(|mb| mb <= 512), "{least:?}");
    let pic = big_picture(&dir);
    let viewer = component("org.kalem.image-viewer", "Image viewer", &["png"]);
    table(
        &measure_picture(Arc::new(kalem_plugin_image_viewer::ImageViewer), &pic),
        &measure_picture(viewer, &pic),
    );
    let pdf = std::env::var_os("KALEM_SPEED_PDF").map_or_else(|| long_pdf(&dir), PathBuf::from);
    let viewer = component("org.kalem.pdf-viewer", "PDF viewer", &["pdf"]);
    table(
        &measure_pdf(Arc::new(kalem_plugin_pdf_viewer::PdfViewer), &pdf),
        &measure_pdf(viewer, &pdf),
    );
    let _ = std::fs::remove_dir_all(&dir);
}
