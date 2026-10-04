//! The `spreadsheet-viewer` world end to end: the fake spreadsheet of
//! `tests/plugins/sheet`, a viewer of the Rust contract exported with the
//! `grid` interface, opened through the host as the contract again, so
//! every value crosses the boundary both ways (T3.7.4). Skipped where the
//! `wasm32-unknown-unknown` target or `wasm-tools` is not installed.

use std::sync::Arc;

mod common;

use kalem_script::Host;
use kalem_script::viewer::{ComponentViewer, VIEWER_LIMITS};
use kalem_viewer::{
    AxisFont, ChartKind, CondRule, CondStyle, ErrorStyle, MacroQuestion, MacroUi, Paint,
    Validation, ValidationKind, Viewer,
};

/// Answers a macro's questions, or none.
struct Ui(Option<i64>, Option<Option<String>>);

impl MacroUi for Ui {
    fn message(&mut self, _prompt: &str, _buttons: i64, _title: &str) -> Option<i64> {
        self.0
    }

    fn input(&mut self, _prompt: &str, _title: &str, _default: &str) -> Option<Option<String>> {
        self.1.clone()
    }
}

#[test]
fn a_spreadsheet_component_is_the_contract_again() {
    let Some(bytes) = common::component("sheet") else {
        return;
    };
    let dir = std::env::temp_dir().join(format!("kalem-grid-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let wasm = dir.join("sheet.wasm");
    std::fs::write(&wasm, bytes).unwrap();
    let book = dir.join("a.sheet");
    std::fs::write(&book, "Name\tValue\nA\t1\nB\t2\n").unwrap();
    let host = Arc::new(Host::new(None).unwrap());
    let v = ComponentViewer::new(
        host,
        &wasm,
        "sheet",
        "Sheet",
        &["sheet".into()],
        VIEWER_LIMITS,
    );
    let mut d = v.open(kalem_viewer::FileHandle::new(&book)).unwrap();

    let l = d.grid(0).expect("a grid");
    assert_eq!((l.rows, l.cols, l.max_rows, l.frozen), (3, 2, 1000, (1, 0)));
    assert_eq!(l.merged, [[0, 0, 0, 1]]);
    assert_eq!(l.filter, Some([0, 0, 2, 1]));
    assert_eq!(l.heights, [(0, 20.0)]);

    let cells = d.grid_cells(0, 0..3, 0..2);
    assert_eq!(cells.len(), 6);
    let (r, c, head) = &cells[0];
    assert_eq!(
        (*r, *c, head.text.as_str(), head.bold),
        (0, 0, "Name", true)
    );
    assert_eq!(head.fill, Some([200, 220, 255]));
    assert_eq!(head.borders, [Some([0, 0, 0]), None, Some([1, 2, 3]), None]);
    assert_eq!(head.border_thick, [true, false, false, true]);
    let one = cells.iter().find(|(r, c, _)| (*r, *c) == (1, 1)).unwrap();
    assert!(one.2.numeric);
    assert_eq!(one.2.bar, Some((500, [0, 128, 0])));

    // Edits, refused with the plugin's reason, undone.
    assert_eq!(d.set_cell(0, 1, 1, "5"), Ok(vec![0]));
    assert_eq!(d.cell_input(0, 1, 1), "5");
    assert_eq!(d.set_cell(0, 1, 1, "!x").unwrap_err().0, "`!x` is refused");
    assert!(d.has_history());
    assert_eq!(d.undo(), Ok(true));
    assert_eq!(d.cell_input(0, 1, 1), "1");

    // A chart, back as it was made.
    d.insert_chart(0, [0, 0, 2, 1], ChartKind::Line, Some("Values".into()))
        .unwrap();
    let charts = d.charts(0);
    assert_eq!(charts.len(), 1);
    let ch = &charts[0];
    assert_eq!(
        (ch.kind, ch.title.as_deref()),
        (ChartKind::Line, Some("Values"))
    );
    assert_eq!(ch.series[0].values, [Some(1.0), Some(2.0)]);
    assert_eq!(ch.series[0].point_colors, [(1, [255, 0, 0])]);
    assert_eq!(ch.anchor, [0, 3, 10, 8]);
    assert_eq!(ch.background, Paint::Color([250, 250, 250]));
    assert_eq!(
        ch.title_font,
        AxisFont {
            size: Some(14.0),
            bold: true,
            ..AxisFont::default()
        }
    );

    // A conditional format and a validation, read back from the plugin.
    let rule = CondRule::Top {
        count: 3,
        bottom: true,
        percent: false,
    };
    let style = CondStyle {
        fill: Some([1, 2, 3]),
        color: None,
        bold: true,
    };
    d.add_conditional_format(0, [1, 1, 2, 1], rule.clone(), style)
        .unwrap();
    assert_eq!(d.cell_format(0, 1, 1), Some(format!("{rule:?} {style:?}")));
    let validation = Validation {
        kind: ValidationKind::List,
        list: vec!["a".into(), "b".into()],
        prompt: Some(("Pick".into(), "one".into())),
        error: Some((ErrorStyle::Warning, "No".into(), "Not that".into())),
        ..Validation::default()
    };
    d.set_validation(0, [1, 0, 2, 0], Some(validation.clone()))
        .unwrap();
    assert_eq!(d.validation(0, 1, 0), Some(validation));

    // A macro: its questions put to the host's dialogs and the run
    // replayed with the answers; one the host cannot answer comes back.
    assert_eq!(d.macros()[0].name, "Module1.Ask");
    let out = d
        .run_macro("Module1.Ask", &mut Ui(Some(6), Some(Some("Grace".into()))))
        .unwrap();
    assert_eq!(out.output, ["6 Grace"]);
    assert!(out.changed && out.question.is_none());
    let out = d.run_macro("Module1.Ask", &mut Ui(Some(7), None)).unwrap();
    assert_eq!(
        out.question,
        Some(MacroQuestion::Input {
            prompt: "Name?".into(),
            title: "Ask".into(),
            default: "Ada".into(),
        })
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// The names of the functions of `block`, the text after `header` up to
/// its closing brace at the start of a line.
fn functions(source: &str, header: &str) -> std::collections::BTreeSet<String> {
    let start = source.find(header).expect("the block");
    let end = source[start..]
        .find("\n}\n")
        .map_or(source.len(), |i| start + i);
    source[start..end]
        .lines()
        .filter_map(|l| l.strip_prefix("    fn "))
        .filter_map(|l| l.split(['(', '<']).next())
        .map(str::to_string)
        .collect()
}

#[test]
fn every_function_of_the_contract_crosses_to_components() {
    // A function added to `ViewerDocument` and not to the component's
    // wrapper (and so to the WIT and the adapter) would do nothing in a
    // viewer installed as a component: added to all four together.
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let contract = std::fs::read_to_string(root.join("kalem-viewer/src/lib.rs")).unwrap();
    let host = std::fs::read_to_string(root.join("kalem-script/src/viewer.rs")).unwrap();
    let want = functions(&contract, "pub trait ViewerDocument");
    let have = functions(
        &host,
        "impl kalem_viewer::ViewerDocument for ComponentDocument",
    );
    let missing: Vec<&String> = want.difference(&have).collect();
    assert!(
        missing.is_empty(),
        "not in kalem-script's ComponentDocument (nor the WIT's `grid` and kalem-plugin's adapter): {missing:?}"
    );
}
