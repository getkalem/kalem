//! A document shown again from the background is current for the
//! plugins at once: the cursor's event names it, as its next motion
//! would (a plugin's command queued after its `file.open` found another
//! document current).

use std::cell::RefCell;
use std::rc::Rc;

use gpui::TestAppContext;
use kalem_core::events::{Event, Reply};
use kalem_core::settings::Config;
use kalem_ui::theme::Theme;
use kalem_ui::workspace::Workspace;

#[gpui::test]
fn a_document_shown_again_is_current_for_the_plugins(cx: &mut TestAppContext) {
    let dir = std::env::temp_dir().join(format!("kalem-ui-current-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let (a, b) = (dir.join("a.txt"), dir.join("b.txt"));
    std::fs::write(&a, "one\n").unwrap();
    std::fs::write(&b, "two\n").unwrap();
    let settings = dir.join("settings.toml");
    std::fs::write(&settings, "[ui]\nlanguage = \"en\"\n").unwrap();
    let mut shared = kalem_ui::shared_in(Config::default(), Some(settings));
    shared.projects = RefCell::new(kalem_core::projects::ProjectState::load(Some(
        dir.join("projects.toml"),
    )));
    let shared = Rc::new(shared);
    let heard = Rc::new(RefCell::new(Vec::new()));
    let h = heard.clone();
    shared.bus.borrow_mut().subscribe(None, move |e| {
        if let Event::SelectionChanged { doc, .. } = e {
            h.borrow_mut().push(*doc);
        }
        Reply::Continue
    });
    let (ws, cx) = cx.add_window_view(|window, cx| {
        let e = kalem_ui::editor::open(Some(&a), shared, Theme::light(), cx).unwrap();
        window.focus(&gpui::Focusable::focus_handle(e.read(cx), cx), cx);
        Workspace::new(e, window, cx)
    });
    cx.run_until_parked();
    ws.update_in(cx, |ws, window, cx| ws.open(&b, None, window, cx));
    cx.run_until_parked();
    ws.update_in(cx, |ws, window, cx| ws.open(&a, None, window, cx));
    cx.run_until_parked();
    let current = ws.read_with(cx, |ws, cx| ws.editor.read(cx).doc_id);
    assert_eq!(
        heard.borrow().last().copied(),
        Some(current),
        "{:?}",
        heard.borrow()
    );
    let _ = std::fs::remove_dir_all(&dir);
}
