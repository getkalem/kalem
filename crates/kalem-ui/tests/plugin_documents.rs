//! A plugin's document (`documents`, plugin API 0.2.5) in the graphical
//! editor: shown in an editor of its own when the plugin opens it, written
//! again in place, never saved, and forgotten once closed. In a test
//! binary of its own, as the plugins' documents are the process's.

use std::rc::Rc;

use gpui::TestAppContext;
use kalem_core::Request;
use kalem_core::extensions::{self as x, GeneratedSpec};
use kalem_core::settings::Config;
use kalem_ui::theme::Theme;
use kalem_ui::workspace::{self, Workspace};

#[gpui::test]
fn a_plugins_document_is_shown_rewritten_and_closed(cx: &mut TestAppContext) {
    let dir = std::env::temp_dir().join(format!("kalem-ui-gendoc-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("notes.org");
    std::fs::write(&path, "* Notes\n").unwrap();
    let settings = dir.join("settings.toml");
    std::fs::write(&settings, "[ui]\nlanguage = \"en\"\n").unwrap();
    let mut shared = kalem_ui::shared_in(Config::default(), Some(settings));
    shared.html_clipboard = || None;
    shared.projects = std::cell::RefCell::new(kalem_core::projects::ProjectState::load(Some(
        dir.join("projects.toml"),
    )));
    let shared = Rc::new(shared);
    let (ws, cx) = cx.add_window_view(|window, cx| {
        let e = kalem_ui::editor::open(Some(&path), shared, Theme::light(), cx).unwrap();
        window.focus(&gpui::Focusable::focus_handle(e.read(cx), cx), cx);
        Workspace::new(e, window, cx)
    });
    cx.run_until_parked();
    let spec = || GeneratedSpec {
        id: "gentest.status".into(),
        key: dir.display().to_string(),
        title: "Git: notes".into(),
        kind: "gentest-status".into(),
        language: Some("diff".into()),
    };
    // What the editors' loop does with the plugins' requests and writes.
    let ask = |r: Request, cx: &mut gpui::VisualTestContext| {
        cx.cx.update(|app| workspace::ask_queued(vec![r], app));
        cx.run_until_parked();
    };
    let written = |cx: &mut gpui::VisualTestContext| {
        cx.cx.update(workspace::generated_written);
        cx.run_until_parked();
    };
    let active = |cx: &mut gpui::VisualTestContext| {
        ws.read_with(cx, |ws, cx| {
            let e = ws.editor.read(cx);
            (
                e.doc.generated.as_ref().map(|g| g.number),
                e.doc.text().as_str().to_string(),
                e.title(),
                ws.editors.len(),
            )
        })
    };

    let text = "Head: main\nUnstaged changes (1)\nmodified notes.org\n";
    let n = x::open_generated("gentest", spec(), text.into(), Some(11)).unwrap();
    ask(Request::ShowGenerated(n), cx);
    assert_eq!(
        active(cx),
        (Some(n), text.to_string(), "Git: notes".to_string(), 2)
    );
    let (head, kind, read_only) = ws.read_with(cx, |ws, cx| {
        let d = &ws.editor.read(cx).doc;
        (d.selection.head, d.text_type(), d.read_only)
    });
    assert_eq!(
        (head, kind.as_str(), read_only),
        (11, "gentest-status", true)
    );

    // Written again: the same editor, the new text.
    let more = "Head: main\nUnstaged changes (1)\nmodified notes.org\n@@ -1 +1 @@\n";
    x::set_generated("gentest", n, more.into(), None).unwrap();
    written(cx);
    assert_eq!(
        active(cx),
        (Some(n), more.to_string(), "Git: notes".to_string(), 2)
    );

    // Not saved: there is no file.
    ws.update_in(cx, |ws, window, cx| {
        ws.editor.update(cx, |e, cx| {
            e.run_command("app.save", serde_json::Value::Null, window, cx)
        })
    });
    cx.run_until_parked();
    let status = ws.read_with(cx, |ws, cx| ws.editor.read(cx).status.clone());
    assert!(
        status
            .as_ref()
            .is_some_and(|(m, _)| m.contains("no file to save")),
        "{status:?}"
    );

    // The plugin closes it: forgotten, the file shows again.
    x::close_generated("gentest", n);
    ask(Request::CloseGenerated(n), cx);
    let (shown, _, title, editors) = active(cx);
    assert_eq!((shown, title.as_str(), editors), (None, "notes.org", 1));
    assert_eq!(x::generated(n), None);

    // The user closes it: forgotten, the plugin's write refused.
    let n = x::open_generated("gentest", spec(), text.into(), None).unwrap();
    ask(Request::ShowGenerated(n), cx);
    assert_eq!(active(cx).0, Some(n));
    ask(Request::Close, cx);
    assert_eq!(active(cx).0, None);
    assert!(x::set_generated("gentest", n, "late".into(), None).is_err());
    let _ = std::fs::remove_dir_all(&dir);
}

/// A plugin's keys in its own documents (`textType == gentest-keys`) come
/// before the profile's and Vim's: Tab and a letter are the plugin's
/// there, in both profiles, and nowhere else.
#[gpui::test]
fn a_plugins_keys_apply_in_its_documents(cx: &mut TestAppContext) {
    use kalem_core::command::{Command, CommandHandler, CommandSource, Scope};
    let dir = std::env::temp_dir().join(format!("kalem-ui-genkeys-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("notes.txt");
    std::fs::write(&path, "one\ntwo\n").unwrap();
    let settings = dir.join("settings.toml");
    std::fs::write(
        &settings,
        "[ui]\nlanguage = \"en\"\n\n[editor]\nkeymap_profile = \"vim\"\n",
    )
    .unwrap();
    // The plugin: a command of its own (its kind is known by it), and its
    // keys in its documents.
    x::add_command(Command {
        id: "gentest.noop".into(),
        title: "Nothing".into(),
        category: String::new(),
        default_keys: Vec::new(),
        when: None,
        handler: CommandHandler::Plugin("gentest".into()),
        args_schema: None,
        source: CommandSource::Plugin("gentest".into()),
        scope: Some(Scope::only(&["gentest-keys"])),
    })
    .unwrap();
    let when = "textType == gentest-keys && (vimCommand || !vimActive)";
    x::add_binding(9_201, "tab", "edit.selectAll", Some(when)).unwrap();
    x::add_binding(9_202, "s", "edit.selectAll", Some(when)).unwrap();
    let config = Config::load(Some(&settings), None);
    let mut shared = kalem_ui::shared_in(config, Some(settings));
    shared.html_clipboard = || None;
    shared.projects = std::cell::RefCell::new(kalem_core::projects::ProjectState::load(Some(
        dir.join("projects.toml"),
    )));
    let shared = Rc::new(shared);
    let (ws, cx) = cx.add_window_view(|window, cx| {
        let e = kalem_ui::editor::open(Some(&path), shared, Theme::light(), cx).unwrap();
        window.focus(&gpui::Focusable::focus_handle(e.read(cx), cx), cx);
        Workspace::new(e, window, cx)
    });
    cx.run_until_parked();
    let spec = GeneratedSpec {
        id: "gentest.keys".into(),
        key: "k".into(),
        title: "Keys".into(),
        kind: "gentest-keys".into(),
        language: None,
    };
    let n = x::open_generated("gentest", spec, "alpha\nbeta\n".into(), None).unwrap();
    cx.cx
        .update(|app| workspace::ask_queued(vec![Request::ShowGenerated(n)], app));
    cx.run_until_parked();
    let selection = |cx: &mut gpui::VisualTestContext| {
        ws.read_with(cx, |ws, cx| {
            let d = &ws.editor.read(cx).doc;
            (
                d.generated.is_some(),
                d.selection.anchor.min(d.selection.head),
                d.selection.anchor.max(d.selection.head),
            )
        })
    };
    assert_eq!(selection(cx), (true, 0, 0));
    for key in ["tab", "s"] {
        ws.update(cx, |ws, cx| {
            ws.editor.update(cx, |e, _| e.doc.move_cursor(0, false))
        });
        cx.simulate_keystrokes(key);
        cx.run_until_parked();
        assert_eq!(selection(cx), (true, 0, "alpha\nbeta\n".len()), "{key}");
    }
    // Elsewhere, `s` is Vim's (substitute): no selection of everything.
    x::close_generated("gentest", n);
    cx.cx
        .update(|app| workspace::ask_queued(vec![Request::CloseGenerated(n)], app));
    cx.run_until_parked();
    cx.simulate_keystrokes("escape");
    let (generated, start, end) = selection(cx);
    assert!(!generated);
    assert_eq!(end - start, 0);
    x::remove_binding(9_201);
    x::remove_binding(9_202);
    x::remove_command("gentest.noop");
    let _ = std::fs::remove_dir_all(&dir);
}

#[gpui::test]
fn a_plugins_marks_reach_the_open_file(cx: &mut TestAppContext) {
    use kalem_core::GutterMark as M;
    let dir = std::env::temp_dir().join(format!("kalem-ui-marks-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("a.txt");
    std::fs::write(&path, "one\ntwo\nthree\n").unwrap();
    let settings = dir.join("settings.toml");
    std::fs::write(&settings, "[ui]\nlanguage = \"en\"\n").unwrap();
    let mut shared = kalem_ui::shared_in(Config::default(), Some(settings));
    shared.html_clipboard = || None;
    let shared = Rc::new(shared);
    let mut editor = None;
    let (_ws, cx) = cx.add_window_view(|window, cx| {
        let e = kalem_ui::editor::open(Some(&path), shared, Theme::light(), cx).unwrap();
        window.focus(&gpui::Focusable::focus_handle(e.read(cx), cx), cx);
        editor = Some(e.clone());
        Workspace::new(e, window, cx)
    });
    cx.run_until_parked();
    let e = editor.unwrap();
    x::set_gutter("gentest", &path, vec![(1, M::Removed), (3, M::Added)]).unwrap();
    cx.cx.update(workspace::gutters_written);
    cx.run_until_parked();
    let marks = e.read_with(cx, |e, _| {
        (0..3).map(|l| e.doc.gutter_mark(l)).collect::<Vec<_>>()
    });
    assert_eq!(marks, [Some(M::Removed), None, Some(M::Added)]);
    // Edited, they go with their lines.
    e.update(cx, |e, cx| {
        e.doc.move_cursor(0, false);
        e.doc.insert_text("zero\n", std::time::Instant::now());
        e.after_change(cx);
    });
    let marks = e.read_with(cx, |e, _| {
        (0..4).map(|l| e.doc.gutter_mark(l)).collect::<Vec<_>>()
    });
    assert_eq!(marks, [None, Some(M::Removed), None, Some(M::Added)]);
    x::clear_gutter("gentest", None);
    let _ = std::fs::remove_dir_all(&dir);
}

/// A plugin's menu (the git plugin's Git menu) stands in the menu bar for
/// a document whose folder is under its version control, and not for
/// another.
#[test]
fn a_plugins_menu_shows_in_a_repository() {
    use kalem_core::command::{Command, CommandHandler, CommandSource, Scope};
    let dir = std::env::temp_dir().join(format!("kalem-ui-gitmenu-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("repo/.git")).unwrap();
    std::fs::create_dir_all(dir.join("plain")).unwrap();
    std::fs::write(dir.join("repo/a.txt"), "a\n").unwrap();
    std::fs::write(dir.join("plain/b.txt"), "b\n").unwrap();
    x::add_command(Command {
        id: "gitmenu.status".into(),
        title: "Gitmenu: Status".into(),
        category: "Gitmenu".into(),
        default_keys: Vec::new(),
        when: None,
        handler: CommandHandler::Plugin("gitmenu".into()),
        args_schema: None,
        source: CommandSource::Plugin("gitmenu".into()),
        scope: Some(Scope::all()),
    })
    .unwrap();
    x::add_menu(x::PluginMenu {
        plugin: "gitmenu".into(),
        title: "Gitmenu".into(),
        when: Some(kalem_core::when::WhenClause::parse("vcs == git").unwrap()),
        items: vec!["gitmenu.status".into()],
    });
    let registry = kalem_core::CommandRegistry::with_builtins();
    let shows = |file: &str| {
        let doc = kalem_core::DocumentState::open(
            &dir.join(file),
            std::sync::Arc::new(org_model::Settings::default()),
            &Default::default(),
        )
        .unwrap();
        workspace::menus_for(&registry, &doc.document_context())
            .iter()
            .any(|m| m.name.as_ref() == "Gitmenu")
    };
    assert!(shows("repo/a.txt"));
    assert!(!shows("plain/b.txt"));
    x::remove_menus("gitmenu");
    x::remove_command("gitmenu.status");
    let _ = std::fs::remove_dir_all(&dir);
}
