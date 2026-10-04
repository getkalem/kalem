#![allow(clippy::print_stderr)]
//! The xlsx plugin built as a component against the same plugin bundled
//! natively (T3.7.4): the same workbooks opened by both, read and edited
//! alike. Runs when `KALEM_XLSX_COMPONENT` names the built component
//! (`kalem plugin build` in getkalem/plugins' `plugins/xlsx`) and
//! `KALEM_XLSX_CORPUS` a folder of workbooks; skipped otherwise.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use kalem_viewer::{FileHandle, Viewer, ViewerDocument};

fn workbooks() -> Option<(PathBuf, Vec<PathBuf>)> {
    let component = PathBuf::from(std::env::var_os("KALEM_XLSX_COMPONENT")?);
    let corpus = PathBuf::from(std::env::var_os("KALEM_XLSX_CORPUS")?);
    let mut books: Vec<PathBuf> = std::fs::read_dir(&corpus)
        .ok()?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|e| e == "xlsx" || e == "xlsm"))
        .collect();
    books.sort();
    Some((component, books))
}

/// What a document shows, every part the grid reads.
fn picture(d: &mut dyn ViewerDocument) -> String {
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

fn first_difference(a: &str, b: &str) -> String {
    for (i, (x, y)) in a.lines().zip(b.lines()).enumerate() {
        if x != y {
            return format!("line {i}:\n  native:    {x}\n  component: {y}");
        }
    }
    format!("lengths {} and {}", a.lines().count(), b.lines().count())
}

#[test]
fn the_component_reads_and_edits_as_the_bundled_plugin() {
    let Some((component, books)) = workbooks() else {
        eprintln!("KALEM_XLSX_COMPONENT and KALEM_XLSX_CORPUS not set: skipped");
        return;
    };
    let host = Arc::new(kalem_script::Host::new(None).unwrap());
    let comp = kalem_script::viewer::ComponentViewer::new(
        host,
        &component,
        "xlsx",
        "Excel workbooks",
        &["xlsx".into(), "xlsm".into()],
        kalem_script::viewer::VIEWER_LIMITS,
    );
    let native = kalem_plugin_xlsx::XlsxViewer;
    assert!(!books.is_empty(), "no workbooks");
    for book in &books {
        let open = |v: &dyn Viewer| v.open(FileHandle::new(Path::new(book))).unwrap();
        let mut a = open(&native);
        let mut b = open(&comp);
        let (pa, pb) = (picture(&mut *a), picture(&mut *b));
        assert!(
            pa == pb,
            "{}: {}",
            book.display(),
            first_difference(&pa, &pb)
        );
        // An edit of the first sheet: a value and a formula over it, the
        // formulas recalculated by IronCalc on both sides.
        if a.grid(0).is_some() {
            for d in [&mut a, &mut b] {
                d.set_cell(0, 20, 0, "21").unwrap();
                d.set_cell(0, 20, 1, "=A21*2+SUM(A21:A21)").unwrap();
            }
            let (pa, pb) = (picture(&mut *a), picture(&mut *b));
            assert!(
                pa == pb,
                "{} after edits: {}",
                book.display(),
                first_difference(&pa, &pb)
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
            let (pa, pb) = (picture(&mut *a), picture(&mut *b));
            assert!(
                pa == pb,
                "{} after a chart: {}",
                book.display(),
                first_difference(&pa, &pb)
            );
            let cell = |d: &mut Box<dyn ViewerDocument>| {
                d.grid_cells(0, 20..21, 1..2)
                    .first()
                    .map(|c| c.2.text.clone())
            };
            assert_eq!(cell(&mut b).as_deref(), Some("63"), "{}", book.display());
            // Saved alike.
            let (sa, sb) = (a.save().unwrap(), b.save().unwrap());
            assert_eq!(sa.losses, sb.losses, "{}", book.display());
            assert_eq!(sa.bytes.len(), sb.bytes.len(), "{}", book.display());
        }
        eprintln!("{}: alike", book.display());
    }
}

#[test]
fn a_component_built_for_another_api_falls_back_to_the_bundled_viewer() {
    // `KALEM_STALE_COMPONENT`: a component built against an older WIT
    // (xlsx 0.0.1 before the grid gained functions).
    let Some(stale) = std::env::var_os("KALEM_STALE_COMPONENT").map(PathBuf::from) else {
        return;
    };
    let book = Path::new(env!("CARGO_MANIFEST_DIR")).join("../kalem-tui/tests/data/budget.xlsx");
    let host = Arc::new(kalem_script::Host::new(None).unwrap());
    let component = || {
        kalem_script::viewer::ComponentViewer::new(
            host.clone(),
            &stale,
            "xlsx",
            "Excel workbooks",
            &["xlsx".into()],
            kalem_script::viewer::VIEWER_LIMITS,
        )
    };
    // Alone: refused, saying why.
    let e = component()
        .open(FileHandle::new(&book))
        .err()
        .expect("refused");
    assert!(
        e.0.contains("another version of Kalem's plugin API"),
        "{}",
        e.0
    );
    // With the bundled viewer behind it: the workbook opens, and the user
    // is told once.
    let said = Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
    let log = said.clone();
    let v = component().with_fallback(
        Some(Arc::new(kalem_plugin_xlsx::XlsxViewer)),
        Arc::new(move |t| log.lock().unwrap().push(t)),
    );
    let mut d = v.open(FileHandle::new(&book)).unwrap();
    assert!(d.grid(0).is_some());
    let _ = v.open(FileHandle::new(&book)).unwrap();
    let said = said.lock().unwrap();
    assert_eq!(said.len(), 1, "{said:?}");
    assert!(
        said[0].contains("the bundled viewer opens its files"),
        "{}",
        said[0]
    );
}
