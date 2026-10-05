//! A file that is not text in the graphical editor, through the image
//! viewer plugin (design §11.13, T3.7.2): drawn in the viewer's area, the
//! viewer's keys, the information panel, the next file of the folder, the
//! picture copied and a link inserted into the document used last.

use std::rc::Rc;
use std::sync::Arc;
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

/// The primary modifier of the Word-like profile on this platform.
fn primary() -> &'static str {
    if cfg!(target_os = "macos") {
        "cmd"
    } else {
        "ctrl"
    }
}

/// A window on `notes.org` in a folder with two pictures.
fn open(
    cx: &mut TestAppContext,
) -> (
    Entity<Workspace>,
    std::path::PathBuf,
    &mut VisualTestContext,
) {
    kalem_core::viewer::register(Arc::new(kalem_plugin_image_viewer::ImageViewer));
    static N: AtomicUsize = AtomicUsize::new(0);
    let n = N.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("kalem-ui-viewer-{}-{n}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    image::RgbaImage::from_pixel(40, 20, image::Rgba([255, 0, 0, 255]))
        .save(dir.join("a.png"))
        .unwrap();
    image::RgbaImage::from_pixel(20, 40, image::Rgba([0, 0, 255, 255]))
        .save(dir.join("b.png"))
        .unwrap();
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
    (ws, dir, vcx)
}

/// The viewer's status: size, zoom, unit.
fn status(ws: &Entity<Workspace>, cx: &mut VisualTestContext) -> String {
    let e = ws.read_with(cx, |ws, _| ws.editor.clone());
    e.update(cx, |e, _| {
        e.doc
            .viewer
            .as_deref_mut()
            .map(|v| v.status())
            .unwrap_or_default()
    })
}

fn mode(ws: &Entity<Workspace>, cx: &mut VisualTestContext) -> DocumentMode {
    ws.read_with(cx, |ws, cx| ws.editor.read(cx).doc.meta.mode.clone())
}

#[gpui::test]
fn a_picture_opens_in_the_viewer(cx: &mut TestAppContext) {
    let (ws, dir, cx) = open(cx);
    ws.update_in(cx, |ws, window, cx| {
        ws.open(&dir.join("a.png"), None, window, cx)
    });
    cx.run_until_parked();
    assert_eq!(mode(&ws, cx), DocumentMode::Viewer);
    let area = cx.debug_bounds("viewer").expect("the viewer's area");
    assert!(area.size.width > gpui::px(40.));
    assert!(
        status(&ws, cx).starts_with("40 × 20 · 100%"),
        "{}",
        status(&ws, cx)
    );

    // Zoom in, actual size, turn.
    cx.simulate_keystrokes("=");
    cx.run_until_parked();
    assert!(status(&ws, cx).contains("125%"), "{}", status(&ws, cx));
    cx.simulate_keystrokes("1 r");
    cx.run_until_parked();
    assert!(
        status(&ws, cx).starts_with("20 × 40"),
        "{}",
        status(&ws, cx)
    );

    // The information panel.
    assert!(cx.debug_bounds("viewer-info").is_none());
    cx.simulate_keystrokes("i");
    cx.run_until_parked();
    assert!(cx.debug_bounds("viewer-info").is_some());

    // Copy: the picture, as PNG.
    cx.simulate_keystrokes(&format!("{}-c", primary()));
    let copied = cx.read_from_clipboard().expect("a clipboard item");
    let png = copied
        .entries()
        .iter()
        .find_map(|e| match e {
            gpui::ClipboardEntry::Image(i) => Some(i.bytes().to_vec()),
            _ => None,
        })
        .expect("an image");
    let img = image::load_from_memory(&png).unwrap();
    assert_eq!((img.width(), img.height()), (20, 40));

    // The next picture of the folder, in the same document.
    cx.simulate_keystrokes("n");
    cx.run_until_parked();
    let (count, name) = ws.read_with(cx, |ws, cx| {
        let e = ws.editor.read(cx);
        (
            ws.editors.len(),
            e.doc
                .meta
                .path
                .as_ref()
                .unwrap()
                .file_name()
                .unwrap()
                .to_owned(),
        )
    });
    assert_eq!(count, 2);
    assert_eq!(name, "b.png");
}

#[gpui::test]
fn insert_link_at_point(cx: &mut TestAppContext) {
    let (ws, dir, cx) = open(cx);
    ws.update_in(cx, |ws, window, cx| {
        ws.open(&dir.join("a.png"), None, window, cx)
    });
    cx.run_until_parked();
    cx.simulate_keystrokes("shift-l");
    cx.run_until_parked();
    assert_eq!(mode(&ws, cx), DocumentMode::Org);
    let text = ws.read_with(cx, |ws, cx| {
        ws.editor.read(cx).doc.text().as_str().to_string()
    });
    assert!(text.contains("[[file:a.png]]"), "{text}");
}

#[gpui::test]
fn a_file_without_a_viewer_is_still_refused(cx: &mut TestAppContext) {
    let (ws, dir, cx) = open(cx);
    std::fs::write(dir.join("x.bin"), [0u8, 1, 2, 3, 0, 0, 9]).unwrap();
    ws.update_in(cx, |ws, window, cx| {
        ws.open(&dir.join("x.bin"), None, window, cx)
    });
    cx.run_until_parked();
    assert_eq!(mode(&ws, cx), DocumentMode::Org);
}

#[gpui::test]
fn vim_command_line_opens_on_a_viewer(cx: &mut TestAppContext) {
    // With Vim's keys, `:` on a picture opens the command line (`:q`
    // closes it as any file); the viewer's own keys stay its.
    let (ws, dir, cx) = open(cx);
    let pic = dir.join("a.png");
    ws.update_in(cx, |ws, window, cx| ws.open(&pic, None, window, cx));
    cx.run_until_parked();
    let e = ws.read_with(cx, |ws, _| ws.editor.clone());
    e.update(cx, |e, _| {
        e.vim = Some(kalem_core::vim::Vim::new());
    });
    assert_eq!(mode(&ws, cx), DocumentMode::Viewer);
    cx.simulate_keystrokes(":");
    cx.run_until_parked();
    assert!(e.read_with(cx, |e, _| e.vim.as_ref().unwrap().command_line.is_some()));
    cx.simulate_keystrokes("q");
    let line = e.read_with(cx, |e, _| e.vim.as_ref().unwrap().command_line.clone());
    assert_eq!(line.as_deref(), Some(":q"));
    cx.simulate_keystrokes("escape");
    let _ = std::fs::remove_dir_all(&dir);
}
