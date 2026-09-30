//! CSV files as spreadsheets save them (T2.7d.7, T2.7d.8): each file in
//! `tests/csv` opens with its dialect found, edits one cell and saves with
//! every other byte as it was (the byte order mark, CR LF, the delimiter,
//! the quoting of untouched records); RFC 4180 fields round trip; a
//! 100,000-row file lays out without reading every record.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use kalem_core::csv;
use kalem_core::{DocumentMode, DocumentState};

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../../tests/csv")).join(name)
}

/// Opens a copy of `name`, sets field `col` of record `row` to `value` and
/// saves: the bytes read and written.
fn edit_cell(name: &str, row: usize, col: usize, value: &str) -> (Vec<u8>, Vec<u8>) {
    let dir = std::env::temp_dir().join(format!("kalem-csv-{}-{name}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(name);
    let before = std::fs::read(fixture(name)).unwrap();
    std::fs::write(&path, &before).unwrap();
    let base = kalem_core::settings::Config::default().parse_base();
    let mut d =
        DocumentState::open(&path, Arc::new(org_model::Settings::default()), &base).unwrap();
    assert_eq!(d.meta.mode, DocumentMode::Csv);
    let l = csv::layout(&d);
    let text = d.text().as_str().to_string();
    let rec = l.index.borrow_mut().record(&text, row, &l.dialect).unwrap();
    let tx = csv::set_cell(&text, &rec, col, value, &l.dialect);
    d.apply(&tx, org_edit::ChangeKind::Command, Instant::now());
    d.save(kalem_core::files::SaveOptions::default(), false)
        .unwrap();
    (before, std::fs::read(&path).unwrap())
}

#[test]
fn excel_turkish_locale() {
    // Excel in a Turkish locale: a byte order mark, CR LF, `;`, decimal
    // commas, dates as text, numbers kept as text in quotes.
    let (before, after) = edit_cell("excel-tr.csv", 2, 1, "99,5");
    let (b, a) = (
        String::from_utf8(before).unwrap(),
        String::from_utf8(after).unwrap(),
    );
    assert_eq!(a, b.replace("\"12,75\"", "99,5"));
    assert!(a.starts_with('\u{feff}'));
    // The column's numbers read with decimal commas.
    let text = b.trim_start_matches('\u{feff}');
    let d = csv::detect(text);
    assert_eq!((d.delimiter, d.header, d.crlf), (b';', true, true));
    let (n, sum, ..) = csv::column_stats(text, &d, 1).unwrap();
    assert_eq!(n, 3);
    assert!((sum - 1243.75).abs() < 1e-9, "{sum}");
}

#[test]
fn libreoffice_quoted_text() {
    // LibreOffice with "Quote all text cells": the untouched records keep
    // their quotes; the edited field is quoted only when it needs it.
    let (before, after) = edit_cell("libreoffice.csv", 3, 3, "a, b");
    let (b, a) = (
        String::from_utf8(before).unwrap(),
        String::from_utf8(after).unwrap(),
    );
    assert_eq!(
        a,
        b.replace(
            "\"Cem\",\"\",2026-10-01,\"\"",
            "\"Cem\",\"\",2026-10-01,\"a, b\""
        )
    );
    let (_, after) = edit_cell("libreoffice.csv", 1, 1, "4");
    let a = String::from_utf8(after).unwrap();
    assert!(
        a.contains("\"Ada\",4,2026-09-29,\"said \"\"hi\"\"\"\n"),
        "{a}"
    );
}

#[test]
fn rfc4180_edge_cases() {
    let text = std::fs::read_to_string(fixture("rfc4180.csv")).unwrap();
    let d = csv::detect(&text);
    let rows = csv::rows(&text, &d);
    assert_eq!(
        rows,
        vec![
            vec!["id", "value"],
            vec!["1", "a,b"],
            vec!["2", "x\"y"],
            vec!["3", "line\r\nbreak"],
            vec!["4", ""],
        ]
    );
    // Written back, each value reads as it was.
    let again: Vec<String> = rows
        .iter()
        .map(|r| {
            r.iter()
                .map(|v| csv::encode(v, &d))
                .collect::<Vec<_>>()
                .join(",")
        })
        .collect();
    assert_eq!(again.join("\r\n") + "\r\n", text);
    let (before, after) = edit_cell("rfc4180.csv", 4, 1, "new\nline");
    let (b, a) = (
        String::from_utf8(before).unwrap(),
        String::from_utf8(after).unwrap(),
    );
    // The line break in the field is written as the file's.
    assert_eq!(a, b.replace("4,\r\n", "4,\"new\r\nline\"\r\n"));
}

#[test]
fn large_files_lay_out_lazily() {
    let mut text = String::from("id,name,amount\n");
    for i in 0..100_000 {
        text.push_str(&format!("{i},name {i},{}.5\n", i % 997));
    }
    let start = Instant::now();
    let l = csv::Layout::new(&text);
    let last = text.len() - "99999,name 99999,297.5\n".len();
    let line = last..text.len() - 1;
    let view = csv::line_view(&l, &text, line);
    assert!(view.display().starts_with("99999"), "{}", view.display());
    // Generous for debug builds on slow machines; the release figure is in
    // book/part-5/performance.org.
    assert!(start.elapsed().as_secs() < 5, "{:?}", start.elapsed());
}
