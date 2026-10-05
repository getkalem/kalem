#![allow(clippy::print_stderr)]
//! wasm_todo W7: each viewer built into Kalem as a component against its
//! native copy, the same files opened by both, read, drawn and edited
//! alike: the workbooks of the terminal editor's fixtures and of the
//! plugin's own corpus (and those of `KALEM_XLSX_CORPUS`, a folder, when
//! set), PDF files, and pictures of each kind the image viewer opens.

mod common;

use std::path::{Path, PathBuf};
use std::sync::Arc;

use kalem_viewer::{FileHandle, RenderRequest, Rendered, Viewer, ViewerDocument};

/// The workbooks: the terminal editor's fixtures, the xlsx plugin's
/// corpus, and `KALEM_XLSX_CORPUS`'s.
fn workbooks() -> Vec<PathBuf> {
    let mut dirs = vec![
        common::fixture(""),
        Path::new(common::built_in("org.kalem.xlsx").source).join("tests/corpus"),
    ];
    dirs.extend(std::env::var_os("KALEM_XLSX_CORPUS").map(PathBuf::from));
    let mut books: Vec<PathBuf> = dirs
        .iter()
        .flat_map(|d| std::fs::read_dir(d).into_iter().flatten())
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| {
            p.extension().is_some_and(|e| {
                ["xlsx", "xlsm", "xls", "xlsb", "ods"]
                    .iter()
                    .any(|x| e == *x)
            })
        })
        .collect();
    books.sort();
    books
}

/// What a workbook shows, every part the grid reads.
fn grid_picture(d: &mut dyn ViewerDocument) -> String {
    let mut out = String::new();
    let s = d.structure();
    out.push_str(&format!("{:?}\n", s.units));
    out.push_str(&format!("names {:?}\n", d.defined_names()));
    out.push_str(&format!("macros {:?}\n", d.macros()));
    out.push_str(&format!("hidden {:?}\n", d.hidden_units()));
    for u in 0..s.units.len() {
        let Some(l) = d.grid(u) else {
            out.push_str(&format!("unit {u}: no grid\n"));
            continue;
        };
        out.push_str(&format!("unit {u}: {l:?}\n"));
        for (r, c, cell) in d.grid_cells(u, 0..l.rows.max(1), 0..l.cols.max(1)) {
            out.push_str(&format!(
                "  {r},{c} {cell:?} input={:?} format={:?} note={:?} link={:?} validation={:?}\n",
                d.cell_input(u, r, c),
                d.cell_format(u, r, c),
                d.cell_note(u, r, c),
                d.cell_link(u, r, c),
                d.validation(u, r, c),
            ));
        }
        out.push_str(&format!("  charts {:?}\n", d.charts(u)));
    }
    out
}

/// What a document of pages or pictures shows, but its pixels: units,
/// outline, information, and each unit's size, text, links and edits.
fn page_picture(d: &mut dyn ViewerDocument, search: &str) -> String {
    let mut out = String::new();
    let s = d.structure();
    out.push_str(&format!("{:?}\n{:?}\n", s.units, s.outline));
    out.push_str(&format!("info {:?}\n", d.info()));
    out.push_str(&format!("search {:?}\n", d.search(search)));
    for u in 0..s.units.len() {
        out.push_str(&format!(
            "unit {u}: size {:?} links {:?} edits {:?}\n  text {:?}\n",
            d.size(u),
            d.links(u),
            d.edits(u),
            d.text(u),
        ));
        let text = d.text(u);
        if !text.is_empty() {
            let end = text.char_indices().nth(20).map_or(text.len(), |(i, _)| i);
            out.push_str(&format!("  rects {:?}\n", d.text_rects(u, 0..end)));
        }
    }
    out
}

/// Unit `unit` drawn at `scale`.
fn pixels(d: &mut dyn ViewerDocument, unit: usize, scale: f32) -> (u32, u32, Arc<Vec<u8>>) {
    let r = d
        .render(
            unit,
            RenderRequest {
                scale,
                ..RenderRequest::default()
            },
        )
        .unwrap();
    let Rendered::Bitmap(b) = r;
    (b.width, b.height, b.rgba)
}

fn first_difference(a: &str, b: &str) -> String {
    for (i, (x, y)) in a.lines().zip(b.lines()).enumerate() {
        if x != y {
            return format!("line {i}:\n  native:    {x}\n  component: {y}");
        }
    }
    format!("lengths {} and {}", a.lines().count(), b.lines().count())
}

fn alike(what: &str, a: &str, b: &str) {
    assert!(a == b, "{what}: {}", first_difference(a, b));
}

/// The same pixels, both drawn the same size.
fn same_pixels(what: &str, a: &mut dyn ViewerDocument, b: &mut dyn ViewerDocument, scale: f32) {
    let units = a.structure().units.len().min(5);
    for u in 0..units {
        let (pa, pb) = (pixels(a, u, scale), pixels(b, u, scale));
        assert_eq!((pa.0, pa.1), (pb.0, pb.1), "{what}: unit {u}'s size");
        let differ = pa.2.iter().zip(pb.2.iter()).filter(|(x, y)| x != y).count();
        assert!(
            differ == 0,
            "{what}: unit {u}: {differ} of {} bytes differ",
            pa.2.len()
        );
    }
}

#[test]
fn the_workbook_component_reads_and_edits_as_the_native_copy() {
    let comp = common::viewer("org.kalem.xlsx");
    let native = kalem_plugin_xlsx::XlsxViewer;
    let books = workbooks();
    assert!(books.len() >= 10, "{books:?}");
    for book in &books {
        let what = book.display().to_string();
        let open = |v: &dyn Viewer| v.open(FileHandle::new(book)).unwrap();
        let mut a = open(&native);
        let mut b = open(&*comp);
        alike(&what, &grid_picture(&mut *a), &grid_picture(&mut *b));
        // An edit of the first sheet: a value and a formula over it, the
        // formulas recalculated by IronCalc on both sides (a workbook
        // only shown, `.xls`, `.xlsb` or `.ods`, is read alike only).
        if a.grid(0).is_some_and(|g| g.editable) {
            for d in [&mut a, &mut b] {
                d.set_cell(0, 20, 0, "21").unwrap();
                d.set_cell(0, 20, 1, "=A21*2+SUM(A21:A21)").unwrap();
            }
            alike(
                &format!("{what} after edits"),
                &grid_picture(&mut *a),
                &grid_picture(&mut *b),
            );
            // A chart inserted, and changed.
            for d in [&mut a, &mut b] {
                d.insert_chart(
                    0,
                    [19, 0, 20, 1],
                    kalem_viewer::ChartKind::Column,
                    Some("T".into()),
                )
                .unwrap();
                let i = d.charts(0).len() - 1;
                d.set_legend(0, i, Some(kalem_viewer::LegendPosition::Right))
                    .unwrap();
            }
            alike(
                &format!("{what} after a chart"),
                &grid_picture(&mut *a),
                &grid_picture(&mut *b),
            );
            let cell = |d: &mut Box<dyn ViewerDocument>| {
                d.grid_cells(0, 20..21, 1..2)
                    .first()
                    .map(|c| c.2.text.clone())
            };
            assert_eq!(cell(&mut b).as_deref(), Some("63"), "{what}");
            // Saved alike.
            let (sa, sb) = (a.save().unwrap(), b.save().unwrap());
            assert_eq!(sa.losses, sb.losses, "{what}");
            assert_eq!(sa.bytes.len(), sb.bytes.len(), "{what}");
        }
        eprintln!("{what}: alike");
    }
}

#[test]
fn the_pdf_component_reads_and_draws_as_the_native_copy() {
    let dir = common::scratch("component-parity-pdf");
    let comp = common::viewer("org.kalem.pdf-viewer");
    let native = kalem_plugin_pdf_viewer::PdfViewer;
    for (pdf, word) in [
        (common::fixture("pages.pdf"), "Page"),
        (common::long_pdf(&dir, 12), "fox"),
    ] {
        let what = pdf.display().to_string();
        let mut a = native.open(FileHandle::new(&pdf)).unwrap();
        let mut b = comp.open(FileHandle::new(&pdf)).unwrap();
        alike(
            &what,
            &page_picture(&mut *a, word),
            &page_picture(&mut *b, word),
        );
        same_pixels(&what, &mut *a, &mut *b, 1.0);
        same_pixels(&what, &mut *a, &mut *b, 1.5);
        eprintln!("{what}: alike");
    }
    let _ = std::fs::remove_dir_all(&dir);
}

/// Pictures of the kinds the image viewer opens, made here: PNG (with
/// transparency), JPEG (turned by its EXIF tag), an animated GIF, BMP and
/// SVG.
fn pictures(dir: &Path) -> Vec<PathBuf> {
    let rgba = image::RgbaImage::from_fn(320, 200, |x, y| {
        image::Rgba([
            (x % 256) as u8,
            (y * 2 % 256) as u8,
            90,
            (x / 2 % 256) as u8,
        ])
    });
    let rgb = image::DynamicImage::ImageRgba8(rgba.clone()).to_rgb8();
    let mut out = Vec::new();
    let png = dir.join("a.png");
    rgba.save(&png).unwrap();
    out.push(png);
    let bmp = dir.join("a.bmp");
    rgb.save(&bmp).unwrap();
    out.push(bmp);
    let mut jpeg = Vec::new();
    image::DynamicImage::ImageRgb8(rgb.clone())
        .write_to(
            &mut std::io::Cursor::new(&mut jpeg),
            image::ImageFormat::Jpeg,
        )
        .unwrap();
    let jpeg = kalem_plugin_image_viewer::jpeg::set_orientation(&jpeg, 6).unwrap();
    let jpg = dir.join("a.jpg");
    std::fs::write(&jpg, jpeg).unwrap();
    out.push(jpg);
    let gif = dir.join("a.gif");
    {
        let file = std::fs::File::create(&gif).unwrap();
        let mut enc = image::codecs::gif::GifEncoder::new(file);
        enc.set_repeat(image::codecs::gif::Repeat::Infinite)
            .unwrap();
        for i in 0..3u8 {
            let frame = image::RgbaImage::from_fn(64, 48, |x, y| {
                image::Rgba([i * 80, (x * 4) as u8, (y * 5) as u8, 255])
            });
            enc.encode_frame(image::Frame::from_parts(
                frame,
                0,
                0,
                image::Delay::from_numer_denom_ms(100, 1),
            ))
            .unwrap();
        }
    }
    out.push(gif);
    out.push(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/kalem.svg"));
    out
}

#[test]
fn the_picture_component_reads_draws_and_turns_as_the_native_copy() {
    let dir = common::scratch("component-parity-pictures");
    let comp = common::viewer("org.kalem.image-viewer");
    let native = kalem_plugin_image_viewer::ImageViewer;
    let mut edited = Vec::new();
    for pic in pictures(&dir) {
        let what = pic.display().to_string();
        let mut a = native.open(FileHandle::new(&pic)).unwrap();
        let mut b = comp.open(FileHandle::new(&pic)).unwrap();
        alike(
            &what,
            &page_picture(&mut *a, ""),
            &page_picture(&mut *b, ""),
        );
        same_pixels(&what, &mut *a, &mut *b, 1.0);
        // Each edit it offers (a JPEG turned by its tag), applied and
        // saved alike.
        let edits = a.edits(0);
        if let Some(e) = edits.first() {
            a.apply(&e.id).unwrap();
            b.apply(&e.id).unwrap();
            alike(
                &format!("{what} after {}", e.id),
                &page_picture(&mut *a, ""),
                &page_picture(&mut *b, ""),
            );
            assert_eq!(a.modified(), b.modified(), "{what}");
            let (sa, sb) = (a.save().unwrap(), b.save().unwrap());
            assert_eq!(sa.losses, sb.losses, "{what}");
            assert!(sa.bytes == sb.bytes, "{what}: saved differently");
            edited.push(what.clone());
        }
        eprintln!("{what}: alike");
    }
    assert!(edited.iter().any(|p| p.ends_with(".jpg")), "{edited:?}");
    let _ = std::fs::remove_dir_all(&dir);
}
