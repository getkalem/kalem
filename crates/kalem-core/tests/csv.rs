#![allow(clippy::print_stderr)]
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
    // Synthetic, in the style of Excel's "CSV UTF-8" in a Turkish locale
    // (Excel is not available to the tests): a byte order mark, CR LF,
    // `;`, decimal commas, dates as text, a field quoted only when it
    // holds the delimiter.
    let (before, after) = edit_cell("excel-tr.csv", 2, 1, "99,5");
    let (b, a) = (
        String::from_utf8(before).unwrap(),
        String::from_utf8(after).unwrap(),
    );
    assert_eq!(a, b.replace("Mehmet;12,75;", "Mehmet;99,5;"));
    assert!(a.starts_with('\u{feff}'));
    // The column's numbers read with decimal commas.
    let text = b.trim_start_matches('\u{feff}');
    let d = csv::detect(text);
    assert_eq!((d.delimiter, d.header, d.crlf), (b';', true, true));
    let (n, sum, ..) = csv::column_stats(text, &d, 1).unwrap();
    assert_eq!(n, 3);
    assert!((sum - 1243.75).abs() < 1e-9, "{sum}");
    assert_eq!(csv::rows(text, &d)[2][3], "a;b");
}

#[test]
fn libreoffice_quoted_text() {
    // Exported by LibreOffice 24.2 from `libreoffice.fods` with "Quote all
    // text cells" (`tools/csv-oracle.sh`): numbers and dates unquoted,
    // text quoted, a line break inside a quoted field. Untouched records
    // keep their quotes; an edited field is quoted only when it needs it.
    let (before, after) = edit_cell("libreoffice.csv", 3, 3, "x, y");
    let (b, a) = (
        String::from_utf8(before).unwrap(),
        String::from_utf8(after).unwrap(),
    );
    assert_eq!(a, b.replace("\"a, b; c\"", "\"x, y\""));
    let (_, after) = edit_cell("libreoffice.csv", 1, 1, "4");
    let a = String::from_utf8(after).unwrap();
    assert!(
        a.contains("\"Ayşe\",4,2026-09-29,\"said \"\"hi\"\"\"\n"),
        "{a}"
    );
    let (_, after) = edit_cell("libreoffice.csv", 3, 0, "Can");
    let a = String::from_utf8(after).unwrap();
    assert!(a.contains("\nCan,,2026-10-01,"), "{a}");
    let text = String::from_utf8(b.into_bytes()).unwrap();
    let d = csv::detect(&text);
    assert_eq!((d.delimiter, d.header, d.crlf), (b',', true, false));
    assert_eq!(csv::rows(&text, &d)[2][3], "two\nlines");
}

#[test]
fn libreoffice_turkish_locale() {
    // Exported by LibreOffice 24.2 from `libreoffice-tr.fods`, cells
    // formatted in Turkish: `;`, grouped numbers with decimal commas,
    // dates as `DD.MM.YYYY`, text quoted only when it needs it.
    let text = std::fs::read_to_string(fixture("libreoffice-tr.csv")).unwrap();
    let d = csv::detect(&text);
    assert_eq!((d.delimiter, d.header, d.crlf), (b';', true, false));
    let (n, sum, ..) = csv::column_stats(&text, &d, 1).unwrap();
    assert_eq!(n, 2);
    assert!((sum - 1238.0).abs() < 1e-9, "{sum}");
    let (before, after) = edit_cell("libreoffice-tr.csv", 2, 1, "7,25");
    let (b, a) = (
        String::from_utf8(before).unwrap(),
        String::from_utf8(after).unwrap(),
    );
    assert_eq!(a, b.replace("Bob;1.234,50;", "Bob;7,25;"));
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
    let view = csv::line_view(&l, &text, line, None);
    assert!(
        view.display().trim_start().starts_with("100001 │    99999"),
        "{}",
        view.display()
    );
    // Generous for debug builds on slow machines; the release figure is in
    // book/part-5/performance.org.
    assert!(start.elapsed().as_secs() < 5, "{:?}", start.elapsed());
}

#[test]
fn dialect_kept_and_set_by_hand() {
    // Detected once: renaming the header cell to a number keeps the
    // header; the commands set the dialect by hand.
    let dir = std::env::temp_dir().join(format!("kalem-csv-dialect-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("d.csv");
    std::fs::write(&path, "name,qty\napple,3\n").unwrap();
    let base = kalem_core::settings::Config::default().parse_base();
    let mut d =
        DocumentState::open(&path, Arc::new(org_model::Settings::default()), &base).unwrap();
    let l = csv::layout(&d);
    assert!(l.dialect.header);
    let text = d.text().as_str().to_string();
    let rec = l.index.borrow_mut().record(&text, 0, &l.dialect).unwrap();
    let tx = csv::set_cell(&text, &rec, 0, "2024", &l.dialect);
    d.apply(&tx, org_edit::ChangeKind::Command, Instant::now());
    assert!(csv::layout(&d).dialect.header, "{}", d.text().as_str());
    let reg = kalem_core::CommandRegistry::with_builtins();
    let config = kalem_core::settings::Config::default();
    let run = |d: &mut DocumentState, id: &str, args: serde_json::Value| {
        let mut clip = kalem_core::command::Clipboard::default();
        let mut ctx = kalem_core::command::EditorContext {
            document: Some(d),
            clipboard: &mut clip,
            config: &config,
            now: Instant::now(),
            clock: jiff::civil::date(2026, 9, 30).at(10, 0, 0, 0),
            messages: Vec::new(),
            requests: Vec::new(),
        };
        reg.execute(id, &mut ctx, &args).unwrap();
    };
    run(&mut d, "csv.toggleHeader", serde_json::json!({}));
    assert!(!csv::layout(&d).dialect.header);
    run(
        &mut d,
        "csv.setDelimiter",
        serde_json::json!({"delimiter": ";"}),
    );
    assert_eq!(csv::layout(&d).dialect.delimiter, b';');
    run(&mut d, "csv.detectDialect", serde_json::json!({}));
    assert_eq!(csv::layout(&d).dialect.delimiter, b',');
    let _ = std::fs::remove_dir_all(&dir);
}

/// A file of 100,000 rows: what the status bar and the view ask at a
/// keystroke and at each step of the cursor, with a filter and a sort on
/// (publish_todo 3.5), each against a ceiling.
#[test]
fn a_large_file_at_each_keystroke_and_step() {
    let dir = std::env::temp_dir().join(format!("kalem-csv-large-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("large.csv");
    let mut text = String::from("id,name,qty,price,city,code,flag,note\n");
    for i in 0..100_000 {
        text.push_str(&format!(
            "{i},item {i},{},{}.{:02},city {},{:05},{},\"note, {i}\"\n",
            i % 97,
            i % 1000,
            i % 100,
            i % 50,
            i * 7 % 100_000,
            i % 2
        ));
    }
    std::fs::write(&path, &text).unwrap();
    let base = kalem_core::settings::Config::default().parse_base();
    let mut d =
        DocumentState::open(&path, Arc::new(org_model::Settings::default()), &base).unwrap();
    assert_eq!(d.meta.mode, DocumentMode::Csv);
    let time = |f: &mut dyn FnMut()| {
        let t = Instant::now();
        f();
        t.elapsed().as_millis()
    };
    let _ = csv::layout(&d);
    // The cursor in the quantities' column, row 500.
    let line = d.text().line_range(500);
    d.selection = org_edit::Selection::caret(line.start + "500,item 500,".len());
    let first = time(&mut || {
        let _ = csv::status(&d);
    });
    // A keystroke: the status bar's numbers again.
    d.insert_text("1", Instant::now());
    let keystroke = time(&mut || {
        let _ = csv::status(&d);
    });
    // Twenty steps down with a filter and a sort on.
    d.csv_filter = Some("city 7".into());
    d.csv_sort = Some((3, false));
    let _ = csv::shown_lines(&d);
    let _ = csv::status(&d);
    let steps = time(&mut || {
        for i in 0..20 {
            let l = d.text().line_range(501 + i);
            d.selection = org_edit::Selection::caret(l.start);
            let _ = csv::shown_lines(&d);
            let _ = csv::status(&d);
        }
    });
    eprintln!("status first {first} ms, after a keystroke {keystroke} ms, 20 steps {steps} ms");
    let debug = cfg!(debug_assertions);
    let (key_ceiling, step_ceiling) = if debug { (2000, 1000) } else { (300, 100) };
    assert!(
        keystroke <= key_ceiling,
        "a keystroke's status: {keystroke} ms"
    );
    assert!(steps <= step_ceiling, "twenty steps: {steps} ms");
    let _ = std::fs::remove_dir_all(&dir);
}
