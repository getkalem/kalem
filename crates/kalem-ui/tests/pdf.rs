//! A PDF file in the graphical editor through the pdf-viewer plugin
//! (T3.7.3): its page fitted to the area and rendered at the scale shown,
//! the next page, the page's text, a link clicked, the outline, the find bar.

use std::rc::Rc;
use std::sync::atomic::{AtomicUsize, Ordering};

use gpui::{Entity, TestAppContext, VisualTestContext};
use kalem_core::DocumentMode;
use kalem_core::settings::Config;
use kalem_ui::theme::Theme;
use kalem_ui::workspace::Workspace;

/// A settings file of the test's own in `dir`, in English: saving or
/// reloading the settings neither touches the user's files nor switches
/// the shared interface language to the system's.
fn test_settings(dir: &std::path::Path) -> Option<std::path::PathBuf> {
    let path = dir.join("settings.toml");
    std::fs::write(&path, "[ui]\nlanguage = \"en\"\n").unwrap();
    Some(path)
}

fn open(cx: &mut TestAppContext) -> (Entity<Workspace>, &mut VisualTestContext) {
    // Compiled before the waits below are timed: on a slow machine the
    // component's compilation takes seconds.
    let pdf = kalem_components::viewer("org.kalem.pdf-viewer").unwrap();
    pdf.plugin().unwrap();
    kalem_core::viewer::register(pdf);
    static N: AtomicUsize = AtomicUsize::new(0);
    let n = N.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("kalem-ui-pdf-{}-{n}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let data =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../kalem-tui/tests/data/pages.pdf");
    std::fs::copy(data, dir.join("pages.pdf")).unwrap();
    std::fs::write(dir.join("notes.org"), "* Notes\n").unwrap();
    let mut shared = kalem_ui::shared_in(Config::default(), test_settings(&dir));
    shared.html_clipboard = || None;
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

/// Waits while a page renders on its thread, the repaint timer fired.
fn settle(ws: &Entity<Workspace>, cx: &mut VisualTestContext) {
    let e = ws.read_with(cx, |ws, _| ws.editor.clone());
    for _ in 0..2000 {
        cx.run_until_parked();
        let busy = e.read_with(cx, |e, _| {
            e.doc
                .viewer
                .as_deref()
                .is_some_and(|v| v.rendering() || v.searching())
        });
        if !busy {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
        cx.executor()
            .advance_clock(std::time::Duration::from_millis(20));
    }
    cx.run_until_parked();
    // The renders end, neighbors included: the editor stops drawing again.
    let busy = e.read_with(cx, |e, _| {
        e.doc
            .viewer
            .as_deref()
            .is_some_and(|v| v.rendering() || v.searching())
    });
    assert!(!busy, "a render never ended");
}

#[gpui::test]
fn a_pdf_opens_page_by_page(cx: &mut TestAppContext) {
    let (ws, cx) = open(cx);
    settle(&ws, cx);
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
    settle(&ws, cx);
    let (status, _, text) = state(&ws, cx);
    assert!(status.ends_with(" · 2/3"), "{status}");
    assert_eq!(text, "Page two");

    // Page one's link (at 72–300 × 72–122 of the page) goes to page three.
    cx.simulate_keystrokes("p");
    settle(&ws, cx);
    let e = ws.read_with(cx, |ws, _| ws.editor.clone());
    let at = e.update(cx, |e, _| {
        let origin = e.viewer_view.bounds.expect("laid out").origin;
        let p = e.doc.viewer.as_deref_mut().unwrap().placement();
        origin
            + gpui::point(
                gpui::px(p.x + 150.0 * p.scale),
                gpui::px(p.y + 97.0 * p.scale),
            )
    });
    cx.simulate_click(at, gpui::Modifiers::default());
    settle(&ws, cx);
    let (status, _, text) = state(&ws, cx);
    assert!(status.ends_with(" · 3/3"), "{status}");
    assert_eq!(text, "Page three");

    // The outline in the sidebar: Section goes to page two.
    let primary = if cfg!(target_os = "macos") {
        "cmd"
    } else {
        "ctrl"
    };
    cx.simulate_keystrokes(&format!("{primary}-shift-o"));
    settle(&ws, cx);
    let section = cx.debug_bounds("outline-1").expect("the row of Section");
    assert!(cx.debug_bounds("outline-2").is_some(), "Chapter 2");
    cx.simulate_click(section.center(), gpui::Modifiers::default());
    settle(&ws, cx);
    let (status, _, text) = state(&ws, cx);
    assert!(status.ends_with(" · 2/3"), "{status}");
    assert_eq!(text, "Page two");

    // The find bar searches the pages on a thread, from the page shown on;
    // Enter goes on, round to page one.
    cx.simulate_keystrokes(&format!("{primary}-f"));
    cx.simulate_input("page");
    settle(&ws, cx);
    let e = ws.read_with(cx, |ws, _| ws.editor.clone());
    let found = |cx: &mut VisualTestContext| {
        e.update(cx, |e, _| {
            let v = e.doc.viewer.as_deref_mut().unwrap();
            (v.unit, v.search_status())
        })
    };
    assert_eq!(found(cx), (1, "2/3".to_string()));
    // The match is marked on the page where "Page" stands, at x 72.
    let (marks, p) = e.update(cx, |e, _| {
        let v = e.doc.viewer.as_deref_mut().unwrap();
        (v.search_marks(), v.placement())
    });
    assert_eq!(marks.len(), 1, "{marks:?}");
    let ([x, _, w, _], shown) = marks[0];
    assert!(shown);
    assert!((x - (p.x + 72.0 * p.scale)).abs() < 1.0, "{x} for {p:?}");
    assert!(w > 50.0 * p.scale, "{w}");
    cx.simulate_keystrokes("enter");
    settle(&ws, cx);
    assert_eq!(found(cx), (2, "3/3".to_string()));
    cx.simulate_keystrokes("enter");
    settle(&ws, cx);
    assert_eq!(found(cx), (0, "1/3".to_string()));
}

/// A one-page PDF whose link leaves the document: a web address at
/// 72–300 × 72–122 of the page, where pages.pdf's link to page three sits.
fn pdf_with_a_web_link(path: &std::path::Path) {
    let content = b"BT /F1 36 Tf 72 680 Td (A link) Tj ET";
    let objects: Vec<Vec<u8>> = vec![
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Contents 4 0 R \
          /Resources << /Font << /F1 5 0 R >> >> /Annots [6 0 R] >>"
            .to_vec(),
        [
            format!("<< /Length {} >>\nstream\n", content.len()).as_bytes(),
            content,
            b"\nendstream",
        ]
        .concat(),
        b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_vec(),
        b"<< /Type /Annot /Subtype /Link /Rect [72 670 300 720] \
          /A << /S /URI /URI (https://example.org/) >> >>"
            .to_vec(),
    ];
    let mut out = b"%PDF-1.7\n".to_vec();
    let mut offsets = Vec::new();
    for (i, body) in objects.iter().enumerate() {
        offsets.push(out.len());
        out.extend_from_slice(format!("{} 0 obj\n", i + 1).as_bytes());
        out.extend_from_slice(body);
        out.extend_from_slice(b"\nendobj\n");
    }
    let xref = out.len();
    let size = objects.len() + 1;
    out.extend_from_slice(format!("xref\n0 {size}\n0000000000 65535 f \n").as_bytes());
    for o in &offsets {
        out.extend_from_slice(format!("{o:010} 00000 n \n").as_bytes());
    }
    out.extend_from_slice(
        format!("trailer\n<< /Size {size} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n").as_bytes(),
    );
    std::fs::write(path, out).unwrap();
}

/// A link that leaves the document (a web address, a file, another
/// program) asks before it opens outside Kalem, its address shown: the
/// link's text need not say where it goes, and a file address can start
/// a program.
#[gpui::test]
fn a_link_outside_the_document_asks_first(cx: &mut TestAppContext) {
    let (ws, cx) = open(cx);
    settle(&ws, cx);
    let e = ws.read_with(cx, |ws, _| ws.editor.clone());
    let dir = e.read_with(cx, |e, _| {
        e.doc
            .meta
            .path
            .clone()
            .unwrap()
            .parent()
            .unwrap()
            .to_path_buf()
    });
    let link = dir.join("link.pdf");
    pdf_with_a_web_link(&link);
    ws.update_in(cx, |ws, window, cx| ws.open(&link, None, window, cx));
    settle(&ws, cx);
    let (_, _, text) = state(&ws, cx);
    assert_eq!(text, "A link");
    let e = ws.read_with(cx, |ws, _| ws.editor.clone());
    let at = e.update(cx, |e, _| {
        let origin = e.viewer_view.bounds.expect("laid out").origin;
        let p = e.doc.viewer.as_deref_mut().unwrap().placement();
        origin
            + gpui::point(
                gpui::px(p.x + 150.0 * p.scale),
                gpui::px(p.y + 97.0 * p.scale),
            )
    });

    // A click asks, with the address; Cancel opens nothing.
    cx.simulate_click(at, gpui::Modifiers::default());
    cx.run_until_parked();
    let (message, detail) = cx.pending_prompt().expect("asked before opening");
    assert!(
        format!("{message} {detail}").contains("https://example.org/"),
        "{message} / {detail}"
    );
    assert_eq!(cx.opened_url(), None);
    cx.simulate_prompt_answer("Cancel");
    cx.run_until_parked();
    assert_eq!(cx.opened_url(), None);

    // Open: the address goes to the system.
    cx.simulate_click(at, gpui::Modifiers::default());
    cx.run_until_parked();
    assert!(cx.has_pending_prompt());
    cx.simulate_prompt_answer("Open");
    cx.run_until_parked();
    assert_eq!(cx.opened_url().as_deref(), Some("https://example.org/"));
}

/// SyncTeX (T2.7h.24): opened at a line, a PDF shows that page; a
/// Ctrl-click (Cmd on macOS) on a page opens the source line typeset
/// there.
#[gpui::test]
fn synctex_both_ways(cx: &mut TestAppContext) {
    let (ws, cx) = open(cx);
    settle(&ws, cx);
    let e = ws.read_with(cx, |ws, _| ws.editor.clone());
    let pdf = e.read_with(cx, |e, _| e.doc.meta.path.clone().unwrap());
    let dir = pdf.parent().unwrap().to_path_buf();
    // Page 2: a line of text from line 1 of notes.org, 72–300 points
    // across, 90–110 down.
    let sp = |bp: f64| (bp * 72.27 / 72.0 * 65536.0).round() as i64;
    std::fs::write(
        dir.join("pages.synctex"),
        format!(
            "SyncTeX Version:1\nInput:1:{}\nUnit:1\nContent:\n{{1\n}}1\n{{2\n(1,1:{},{}:{},{},0\nx1,1:{},{}\n)\n}}2\n",
            dir.join("notes.org").display(),
            sp(72.0),
            sp(110.0),
            sp(228.0),
            sp(20.0),
            sp(80.0),
            sp(110.0)
        ),
    )
    .unwrap();
    // Opened at "line" 2: page 2.
    ws.update_in(cx, |ws, window, cx| ws.open(&pdf, Some((2, 0)), window, cx));
    settle(&ws, cx);
    let (status, _, _) = state(&ws, cx);
    assert!(status.ends_with(" · 2/3"), "{status}");
    // Ctrl-click in that line of text: notes.org.
    let at = e.update(cx, |e, _| {
        let origin = e.viewer_view.bounds.expect("laid out").origin;
        let p = e.doc.viewer.as_deref_mut().unwrap().placement();
        origin
            + gpui::point(
                gpui::px(p.x + 150.0 * p.scale),
                gpui::px(p.y + 100.0 * p.scale),
            )
    });
    cx.simulate_click(at, gpui::Modifiers::secondary_key());
    cx.run_until_parked();
    let path = ws.read_with(cx, |ws, cx| ws.editor.read(cx).doc.meta.path.clone());
    assert_eq!(
        path.and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned())),
        Some("notes.org".to_string())
    );
}

#[gpui::test]
fn text_is_selected_by_dragging_and_copied(cx: &mut TestAppContext) {
    let (ws, cx) = open(cx);
    settle(&ws, cx);
    let e = ws.read_with(cx, |ws, _| ws.editor.clone());
    // Where a point of page one is in the window: "Page one" stands at x
    // 72 on the baseline 112, 36 points high.
    let at = |x: f32, y: f32, cx: &mut VisualTestContext| {
        e.update(cx, |e, _| {
            let origin = e.viewer_view.bounds.expect("laid out").origin;
            let p = e.doc.viewer.as_deref_mut().unwrap().placement();
            origin + gpui::point(gpui::px(p.x + x * p.scale), gpui::px(p.y + y * p.scale))
        })
    };
    let none = gpui::Modifiers::default();
    let (from, to) = (at(76.0, 100.0, cx), at(150.0, 100.0, cx));
    cx.simulate_mouse_down(from, gpui::MouseButton::Left, none);
    cx.simulate_mouse_move(to, Some(gpui::MouseButton::Left), none);
    cx.simulate_mouse_up(to, gpui::MouseButton::Left, none);
    settle(&ws, cx);
    let selected = e.update(cx, |e, _| {
        let v = e.doc.viewer.as_deref_mut().unwrap();
        (v.selected_text(), v.selection_marks().len())
    });
    assert_eq!(selected, (Some("Page".to_string()), 1));
    let primary = if cfg!(target_os = "macos") {
        "cmd"
    } else {
        "ctrl"
    };
    cx.simulate_keystrokes(&format!("{primary}-c"));
    let copied = cx.read_from_clipboard().and_then(|c| c.text());
    assert_eq!(copied.as_deref(), Some("Page"));

    // A click drops it; a drag from where there is no text pans instead.
    let blank = at(400.0, 250.0, cx);
    let up = at(400.0, 200.0, cx);
    cx.simulate_click(blank, none);
    cx.simulate_mouse_down(blank, gpui::MouseButton::Left, none);
    cx.simulate_mouse_move(up, Some(gpui::MouseButton::Left), none);
    cx.simulate_mouse_up(up, gpui::MouseButton::Left, none);
    settle(&ws, cx);
    let selected = e.update(cx, |e, _| e.doc.viewer.as_deref().unwrap().selected_text());
    assert_eq!(selected, None);
}

#[gpui::test]
fn a_double_click_selects_a_word_and_a_triple_one_the_line(cx: &mut TestAppContext) {
    let (ws, cx) = open(cx);
    settle(&ws, cx);
    let e = ws.read_with(cx, |ws, _| ws.editor.clone());
    // Page two (page one's text is a link): on "two" of "Page two" (x 72,
    // baseline 112, 36 points).
    cx.simulate_keystrokes("n");
    settle(&ws, cx);
    let at = e.update(cx, |e, _| {
        let origin = e.viewer_view.bounds.expect("laid out").origin;
        let p = e.doc.viewer.as_deref_mut().unwrap().placement();
        origin
            + gpui::point(
                gpui::px(p.x + 180.0 * p.scale),
                gpui::px(p.y + 100.0 * p.scale),
            )
    });
    let click = |n: usize, cx: &mut VisualTestContext| {
        cx.simulate_mouse_move(at, None, gpui::Modifiers::default());
        cx.simulate_event(gpui::MouseDownEvent {
            button: gpui::MouseButton::Left,
            position: at,
            modifiers: gpui::Modifiers::default(),
            click_count: n,
            first_mouse: false,
        });
        cx.simulate_event(gpui::MouseUpEvent {
            button: gpui::MouseButton::Left,
            position: at,
            modifiers: gpui::Modifiers::default(),
            click_count: n,
        });
        cx.run_until_parked();
    };
    let selected = |cx: &mut VisualTestContext| {
        e.update(cx, |e, _| e.doc.viewer.as_deref().unwrap().selected_text())
    };
    click(1, cx);
    click(2, cx);
    assert_eq!(selected(cx).as_deref(), Some("two"));
    let primary = if cfg!(target_os = "macos") {
        "cmd"
    } else {
        "ctrl"
    };
    cx.simulate_keystrokes(&format!("{primary}-c"));
    assert_eq!(
        cx.read_from_clipboard().and_then(|c| c.text()).as_deref(),
        Some("two")
    );
    click(3, cx);
    assert_eq!(selected(cx).as_deref(), Some("Page two"));
    // A single click drops it.
    click(1, cx);
    assert_eq!(selected(cx), None);
}
