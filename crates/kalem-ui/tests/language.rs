//! Switching the interface to Turkish in the settings panel. In its own
//! test binary: the language is global to the process.

use std::rc::Rc;

use gpui::TestAppContext;
use kalem_core::settings::Config;
use kalem_ui::theme::Theme;
use kalem_ui::workspace::Workspace;

#[gpui::test]
fn switching_to_turkish(cx: &mut TestAppContext) {
    let dir = std::env::temp_dir().join(format!("kalem-ui-language-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("t.org");
    std::fs::write(&path, "* A\none two\n").unwrap();
    let mut shared = kalem_ui::shared_in(Config::default(), Some(dir.join("settings.toml")));
    shared.html_clipboard = || None;
    shared.projects = std::cell::RefCell::new(kalem_core::projects::ProjectState::load(Some(
        dir.join("projects.toml"),
    )));
    let shared = Rc::new(shared);
    let mut editor = None;
    let (_ws, cx) = cx.add_window_view(|window, cx| {
        let e = kalem_ui::editor::open(Some(&path), shared, Theme::light(), cx).unwrap();
        window.focus(&gpui::Focusable::focus_handle(e.read(cx), cx), cx);
        editor = Some(e.clone());
        Workspace::new(e, window, cx)
    });
    let e = editor.unwrap();
    let primary = if cfg!(target_os = "macos") {
        "cmd"
    } else {
        "ctrl"
    };
    cx.simulate_keystrokes(&format!("{primary}-,"));
    cx.simulate_keystrokes("/");
    cx.simulate_input("ui.language");
    // `auto`, `en`, `tr`: two steps.
    cx.simulate_keystrokes("enter l");
    cx.run_until_parked();
    cx.simulate_keystrokes("l");
    cx.run_until_parked();
    assert_eq!(kalem_core::l10n::language(), "tr");
    // Titles, messages and counts in Turkish.
    let palette = e.read_with(cx, |e, _| {
        kalem_core::palette::items(&e.shared.registry, &e.shared.keymap, &e.context(), |k| {
            k.to_string()
        })
    });
    assert!(
        palette
            .iter()
            .any(|i| i.id == "app.save" && i.title == "Kaydet")
    );
    let words = e.read_with(cx, |e, _| e.words.borrow_mut().get(&e.doc));
    let (d, s) = words.expect("counted");
    assert_eq!(
        kalem_core::stats::describe(d, s, Default::default()),
        "3 kelime, bölümde 3"
    );
    let menus = kalem_ui::workspace::menus();
    assert_eq!(menus[1].name.as_ref(), "Dosya");
    let saved = std::fs::read_to_string(dir.join("settings.toml")).unwrap();
    assert!(saved.contains("language = \"tr\""), "{saved}");
    let _ = std::fs::remove_dir_all(dir);
}
