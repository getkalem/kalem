//! A PDF file in the graphical editor through the pdf-viewer plugin
//! (T3.7.3): its page fitted to the area and rendered at the scale shown,
//! the next page, the page's text, a link clicked.

use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use gpui::{Entity, TestAppContext, VisualTestContext};
use kalem_core::DocumentMode;
use kalem_core::settings::Config;
use kalem_ui::theme::Theme;
use kalem_ui::workspace::Workspace;

fn open(cx: &mut TestAppContext) -> (Entity<Workspace>, &mut VisualTestContext) {
    kalem_core::viewer::register(Arc::new(kalem_plugin_pdf_viewer::PdfViewer));
    static N: AtomicUsize = AtomicUsize::new(0);
    let n = N.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("kalem-ui-pdf-{}-{n}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let data =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../kalem-tui/tests/data/pages.pdf");
    std::fs::copy(data, dir.join("pages.pdf")).unwrap();
    std::fs::write(dir.join("notes.org"), "* Notes\n").unwrap();
    let mut shared = kalem_ui::shared(Config::default());
    shared.html_clipboard = || None;
    shared.settings_path = Some(dir.join("settings.toml"));
    shared.projects = std::cell::RefCell::new(kalem_core::projects::ProjectState::load(Some(
        dir.join("projects.toml"),
    )));
    let shared = Rc::new(shared);
    let path = dir.join("notes.org");
    let (ws, vcx) = cx.add_window_view(|window, cx| {
        let e = kalem_ui::editor::open(Some(&path), shared, Theme::light(), cx).unwrap();
        window.focus(&gpui::Focusable::focus_handle(e.read(cx), cx), cx);
        Workspace::new(e, window, cx)
    });
    vcx.run_until_parked();
    let pdf = dir.join("pages.pdf");
    ws.update_in(vcx, |ws, window, cx| ws.open(&pdf, None, window, cx));
    vcx.run_until_parked();
    (ws, vcx)
}

/// The viewer's status, its render scale and the page's text.
fn state(ws: &Entity<Workspace>, cx: &mut VisualTestContext) -> (String, f32, String) {
    let e = ws.read_with(cx, |ws, _| ws.editor.clone());
    e.update(cx, |e, _| {
        let v = e.doc.viewer.as_deref_mut().expect("a viewer");
        (v.status(), v.render_scale(), v.text())
    })
}

#[gpui::test]
fn a_pdf_opens_page_by_page(cx: &mut TestAppContext) {
    let (ws, cx) = open(cx);
    let mode = ws.read_with(cx, |ws, cx| ws.editor.read(cx).doc.meta.mode.clone());
    assert_eq!(mode, DocumentMode::Viewer);
    assert!(cx.debug_bounds("viewer").is_some(), "the page is drawn");
    let (status, scale, text) = state(&ws, cx);
    assert!(status.starts_with("612 × 792 · "), "{status}");
    assert!(status.ends_with(" · 1/3"), "{status}");
    assert_eq!(text, "Page one");
    // Rendered for the pixels it is shown at, not at 72 dpi.
    let shown: f32 = status
        .split(" · ")
        .nth(1)
        .unwrap()
        .trim_end_matches('%')
        .parse()
        .unwrap();
    assert!(scale >= shown / 100.0, "{scale} for {status}");

    cx.simulate_keystrokes("n");
    cx.run_until_parked();
    let (status, _, text) = state(&ws, cx);
    assert!(status.ends_with(" · 2/3"), "{status}");
    assert_eq!(text, "Page two");

    // Page one's link (at 72–300 × 72–122 of the page) goes to page three.
    cx.simulate_keystrokes("p");
    cx.run_until_parked();
    let e = ws.read_with(cx, |ws, _| ws.editor.clone());
    let at = e.update(cx, |e, _| {
        let origin = e.viewer_view.bounds.expect("laid out").origin;
        let p = e.doc.viewer.as_deref_mut().unwrap().placement();
        origin + gpui::point(gpui::px(p.x + 150.0 * p.scale), gpui::px(p.y + 97.0 * p.scale))
    });
    cx.simulate_click(at, gpui::Modifiers::default());
    cx.run_until_parked();
    let (status, _, text) = state(&ws, cx);
    assert!(status.ends_with(" · 3/3"), "{status}");
    assert_eq!(text, "Page three");
}
