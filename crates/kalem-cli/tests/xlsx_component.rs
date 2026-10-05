#![allow(clippy::print_stderr)]
//! A workbook component built for another version of the plugin API
//! (`KALEM_STALE_COMPONENT`, xlsx 0.0.1's) refused, and the built-in one
//! opening its files in its place.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use kalem_viewer::{FileHandle, Viewer};

#[test]
fn a_component_built_for_another_api_falls_back_to_the_built_in_one() {
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
    // With the built-in one behind it: the workbook opens, and the user
    // is told once.
    let said = Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
    let log = said.clone();
    let v = component().with_fallback(
        Some(kalem_components::viewer("org.kalem.xlsx").unwrap()),
        Arc::new(move |t| log.lock().unwrap().push(t)),
    );
    let mut d = v.open(FileHandle::new(&book)).unwrap();
    assert!(d.grid(0).is_some());
    let _ = v.open(FileHandle::new(&book)).unwrap();
    let said = said.lock().unwrap();
    assert_eq!(said.len(), 1, "{said:?}");
    assert!(
        said[0].contains("the one built into Kalem opens its files"),
        "{}",
        said[0]
    );
}
