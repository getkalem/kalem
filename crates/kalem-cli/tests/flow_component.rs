#![allow(clippy::print_stderr)]
//! A real viewer of flowing text as a component (plugin API 0.2.7), end to
//! end: `KALEM_FLOW_COMPONENT` names its component (the docx plugin of
//! getkalem/plugins, built with `kalem plugin build`) and
//! `KALEM_FLOW_FILE` a file it opens. The file opens as a document of the
//! editor through the component, is typed into, undone, typed into again
//! and saved, and opens again with the edit. Skipped without the two.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

#[test]
fn a_flow_component_opens_edits_and_saves() {
    let (Some(component), Some(file)) = (
        std::env::var_os("KALEM_FLOW_COMPONENT").map(PathBuf::from),
        std::env::var_os("KALEM_FLOW_FILE").map(PathBuf::from),
    ) else {
        return;
    };
    let ext = file
        .extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();
    let host = Arc::new(kalem_script::Host::new(None).unwrap());
    let viewer: Arc<dyn kalem_viewer::Viewer> =
        Arc::new(kalem_script::viewer::ComponentViewer::new(
            host,
            &component,
            "flow-under-test",
            "Flow under test",
            &[ext],
            kalem_script::viewer::VIEWER_LIMITS,
        ));
    let dir = std::env::temp_dir().join(format!("kalem-flow-component-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let copy = dir.join(file.file_name().unwrap());
    std::fs::copy(&file, &copy).unwrap();
    let settings = Arc::new(org_model::Settings::default());
    let mut d = kalem_core::DocumentState::viewed(&copy, viewer.clone(), settings.clone()).unwrap();
    assert_eq!(d.meta.mode, kalem_core::DocumentMode::Flow);
    let before = d.text().as_str().to_string();
    eprintln!(
        "{} lines, {} bytes of text",
        before.lines().count(),
        before.len()
    );
    assert!(!before.is_empty());
    // Typed at the start of the first line that has text.
    let at = before
        .lines()
        .scan(0usize, |pos, l| {
            let start = *pos;
            *pos += l.len() + 1;
            Some((start, l))
        })
        .find(|(_, l)| l.chars().next().is_some_and(char::is_alphanumeric))
        .map_or(0, |(s, _)| s);
    d.move_cursor(at, false);
    d.insert_text("Kalem ", Instant::now());
    assert!(d.take_notice().is_none());
    assert!(
        d.text().as_str()[at..].starts_with("Kalem "),
        "{}",
        &d.text().as_str()[at..]
    );
    assert!(d.is_modified());
    assert!(d.undo().is_some());
    assert_eq!(d.text().as_str(), before);
    d.insert_text("Kalem ", Instant::now());
    d.save(Default::default(), true).unwrap();
    assert!(!d.is_modified());
    let again = kalem_core::DocumentState::viewed(&copy, viewer, settings).unwrap();
    assert!(again.text().as_str()[at..].starts_with("Kalem "));
    let _ = std::fs::remove_dir_all(&dir);
}
