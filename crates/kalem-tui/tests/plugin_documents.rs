//! A plugin's document (`documents`, plugin API 0.2.5) in the terminal
//! editor: shown when the plugin opens it, written again in place,
//! never saved, and forgotten once closed. In a test binary of its own, as
//! the plugins' requests are the process's.

use kalem_core::extensions::{self as x, GeneratedSpec};
use kalem_core::settings::Config;
use kalem_tui::app::App;
use kalem_tui::caps::Caps;
use std::sync::Mutex;
use std::time::Instant;

/// The plugins' requests and documents are the process's: a test's tick
/// takes another's requests when they run at once (they did on Windows),
/// so the tests here run one at a time.
static ONE_AT_A_TIME: Mutex<()> = Mutex::new(());

fn one_at_a_time() -> std::sync::MutexGuard<'static, ()> {
    ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner())
}

#[test]
fn a_plugins_document_is_shown_rewritten_and_closed() {
    let _serial = one_at_a_time();
    let dir = std::env::temp_dir().join(format!("kalem-tui-gendoc-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("notes.org"), "* Notes\n").unwrap();
    let mut app = App::with_keymap(
        Some(&dir.join("notes.org")),
        Config::default(),
        Caps::full(),
        &[],
        Vec::new(),
    )
    .unwrap();
    let spec = || GeneratedSpec {
        id: "gentest.status".into(),
        key: dir.display().to_string(),
        title: "Git: notes".into(),
        kind: "gentest-status".into(),
        language: Some("diff".into()),
    };
    let text = "Head: main\nUnstaged changes (1)\nmodified notes.org\n";
    let n = x::open_generated("gentest", spec(), text.into(), Some(11), Vec::new()).unwrap();
    app.tick(Instant::now());
    let shown = |app: &App| app.doc.generated.as_ref().map(|g| g.number);
    assert_eq!(shown(&app), Some(n));
    assert_eq!(app.doc.text().as_str(), text);
    assert_eq!(app.doc.selection.head, 11);
    assert!(app.doc.read_only);
    assert_eq!(app.doc.text_type(), "gentest-status");
    let titles: Vec<String> = app.open_files().into_iter().map(|f| f.title).collect();
    assert_eq!(titles, ["notes.org", "Git: notes"]);

    // Written again: the same document, its new text, the cursor on its
    // line.
    let more = "Head: main\nUnstaged changes (1)\nmodified notes.org\n@@ -1 +1 @@\n-* Notes\n+* All notes\n";
    x::set_generated("gentest", n, more.into(), None, Vec::new()).unwrap();
    app.tick(Instant::now());
    assert_eq!(app.doc.text().as_str(), more);
    assert_eq!(app.doc.text().line_of(app.doc.selection.head), 1);
    assert_eq!(app.open_files().len(), 2);
    // Opened again by the plugin, it is the one shown.
    app.run_command("file.next", serde_json::Value::Null);
    assert_eq!(shown(&app), None);
    let _ = x::open_generated("gentest", spec(), more.into(), None, Vec::new()).unwrap();
    app.tick(Instant::now());
    assert_eq!(shown(&app), Some(n));
    assert_eq!(app.open_files().len(), 2);

    // Not saved: there is no file.
    app.run_command("app.save", serde_json::Value::Null);
    let (m, error) = app.status_message().expect("a message");
    assert!(m.contains("no file to save") && !error, "{m}");
    assert!(!dir.join("Git: notes").exists());

    // The plugin closes it: the file shows again.
    x::close_generated("gentest", n);
    app.tick(Instant::now());
    assert_eq!(shown(&app), None);
    assert_eq!(app.open_files().len(), 1);
    assert_eq!(x::generated(n), None);

    // The user closes it: forgotten, the plugin's write refused.
    let n = x::open_generated("gentest", spec(), text.into(), None, Vec::new()).unwrap();
    app.tick(Instant::now());
    assert_eq!(shown(&app), Some(n));
    app.run_command("file.close", serde_json::Value::Null);
    assert_eq!(shown(&app), None);
    assert_eq!(
        app.doc.meta.path.as_deref(),
        Some(dir.join("notes.org").as_path())
    );
    assert!(x::set_generated("gentest", n, "late".into(), None, Vec::new()).is_err());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_plugins_marks_stand_beside_the_lines() {
    let _serial = one_at_a_time();
    use kalem_core::GutterMark as M;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    let dir = std::env::temp_dir().join(format!("kalem-tui-marks-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("a.txt");
    std::fs::write(&file, "one\ntwo\nthree\nfour\n").unwrap();
    let mut app = App::with_keymap(
        Some(&file),
        Config::default(),
        Caps::full(),
        &[],
        Vec::new(),
    )
    .unwrap();
    let mut term = Terminal::new(TestBackend::new(60, 8)).unwrap();
    let mut rows = |app: &mut App| -> Vec<String> {
        term.draw(|f| app.draw(f)).unwrap();
        let buf = term.backend().buffer().clone();
        (0..buf.area.height)
            .map(|y| {
                (0..buf.area.width)
                    .map(|x| buf[(x, y)].symbol().to_string())
                    .collect::<String>()
            })
            .collect()
    };
    x::set_gutter("gentest", &file, vec![(2, M::Changed), (4, M::Added)]).unwrap();
    app.tick(Instant::now());
    let screen = rows(&mut app);
    let line = |n: &str| {
        screen
            .iter()
            .find(|r| r.contains(n))
            .cloned()
            .unwrap_or_default()
    };
    assert!(line("two").contains("▎"), "{screen:#?}");
    assert!(line("four").contains("▎"), "{screen:#?}");
    assert!(!line("one").contains("▎"), "{screen:#?}");
    // Next Change goes to them; the end says so.
    app.run_command("edit.nextChange", serde_json::Value::Null);
    assert_eq!(app.doc.text().line_of(app.doc.selection.head), 1);
    app.run_command("edit.nextChange", serde_json::Value::Null);
    assert_eq!(app.doc.text().line_of(app.doc.selection.head), 3);
    app.run_command("edit.nextChange", serde_json::Value::Null);
    assert_eq!(app.status_message().map(|m| m.0), Some("No more changes"));
    // Taken away: no column.
    x::clear_gutter("gentest", None);
    app.tick(Instant::now());
    let screen = rows(&mut app);
    assert!(!screen.iter().any(|r| r.contains("▎")), "{screen:#?}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_plugins_document_is_drawn_in_its_styles() {
    let _serial = one_at_a_time();
    use kalem_core::{SpanStyle, StyleColor, StyleSpan};
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use ratatui::style::{Color, Modifier};
    let dir = std::env::temp_dir().join(format!("kalem-tui-styled-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("notes.txt"), "notes\n").unwrap();
    let mut app = App::with_keymap(
        Some(&dir.join("notes.txt")),
        Config::default(),
        Caps::full(),
        &[],
        Vec::new(),
    )
    .unwrap();
    let spec = GeneratedSpec {
        id: "gentest.styled".into(),
        key: dir.display().to_string(),
        title: "Styled".into(),
        kind: "gentest-styled".into(),
        language: None,
    };
    let style = |color, bold| SpanStyle {
        color,
        bold,
        ..SpanStyle::default()
    };
    // ` M a.txt`: the code red, the name green and bold.
    let styles = vec![
        StyleSpan {
            range: 0..2,
            style: style(StyleColor::Red, false),
        },
        StyleSpan {
            range: 3..8,
            style: style(StyleColor::Green, true),
        },
    ];
    let n = x::open_generated("gentest", spec, " M a.txt\n".into(), None, styles).unwrap();
    app.tick(Instant::now());
    assert_eq!(app.doc.generated.as_ref().map(|g| g.number), Some(n));
    let mut term = Terminal::new(TestBackend::new(60, 6)).unwrap();
    term.draw(|f| app.draw(f)).unwrap();
    let buf = term.backend().buffer().clone();
    let (y, x0) = (0..buf.area.height)
        .find_map(|y| {
            let row: String = (0..buf.area.width)
                .map(|x| buf[(x, y)].symbol().to_string())
                .collect();
            row.find("M a.txt")
                .map(|i| (y, row[..i].chars().count() as u16))
        })
        .expect("the document's line");
    let m = &buf[(x0, y)];
    let a = &buf[(x0 + 2, y)];
    let colors = |c: Color| c != Color::Reset;
    assert!(colors(m.fg) && colors(a.fg) && m.fg != a.fg, "{m:?} {a:?}");
    assert!(a.modifier.contains(Modifier::BOLD), "{a:?}");
    assert!(!m.modifier.contains(Modifier::BOLD), "{m:?}");
    x::close_generated("gentest", n);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn marked_lines_are_tinted_under_their_text() {
    let _serial = one_at_a_time();
    use kalem_core::GutterMark as M;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    let dir = std::env::temp_dir().join(format!("kalem-tui-tint-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("a.txt");
    std::fs::write(&file, "one\ntwo\nthree\nfour\n").unwrap();
    // With the theme's colors (a true-color terminal), and with the
    // terminal's palette alone.
    let themed = Caps {
        colors: Some(std::sync::Arc::new(
            kalem_core::theme::ThemeColors::builtin(true),
        )),
        ..Caps::full()
    };
    let palette = Caps {
        true_color: false,
        ..Caps::full()
    };
    for caps in [themed, palette] {
        let mut app =
            App::with_keymap(Some(&file), Config::default(), caps, &[], Vec::new()).unwrap();
        let mut term = Terminal::new(TestBackend::new(60, 8)).unwrap();
        x::set_gutter("gentest", &file, vec![(2, M::Changed), (4, M::Added)]).unwrap();
        app.tick(Instant::now());
        term.draw(|f| app.draw(f)).unwrap();
        let buf = term.backend().buffer().clone();
        let text = |y: u16| {
            (0..buf.area.width)
                .map(|x| buf[(x, y)].symbol().to_string())
                .collect::<String>()
        };
        // The background of the word's first cell.
        let bg_at = |word: &str| {
            let y = (0..buf.area.height)
                .find(|&y| text(y).contains(word))
                .unwrap();
            let first = word.chars().next().unwrap().to_string();
            let x = (0..buf.area.width)
                .find(|&x| buf[(x, y)].symbol() == first)
                .unwrap();
            buf[(x, y)].bg
        };
        // The base is a line without a mark that is not the cursor's (the
        // cursor's line has a shade of its own); the marked ones differ
        // from it and from each other.
        let base = bg_at("three");
        assert_ne!(bg_at("two"), base, "a changed line is tinted");
        assert_ne!(bg_at("four"), base, "an added line is tinted");
        assert_ne!(bg_at("two"), bg_at("four"), "changed and added differ");
        assert_ne!(
            bg_at("two"),
            bg_at("one"),
            "the tint is not the cursor line's shade"
        );
        x::clear_gutter("gentest", None);
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_diffs_lines_have_backgrounds() {
    let _serial = one_at_a_time();
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    let dir = std::env::temp_dir().join(format!("kalem-tui-diffbg-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("notes.txt"), "notes\n").unwrap();
    let caps = Caps {
        colors: Some(std::sync::Arc::new(
            kalem_core::theme::ThemeColors::builtin(true),
        )),
        ..Caps::full()
    };
    let mut app = App::with_keymap(
        Some(&dir.join("notes.txt")),
        Config::default(),
        caps,
        &[],
        Vec::new(),
    )
    .unwrap();
    let spec = GeneratedSpec {
        id: "gentest.diff".into(),
        key: dir.display().to_string(),
        title: "Diff".into(),
        kind: "gentest-diff".into(),
        language: Some("diff".into()),
    };
    let text = "header\n@@ -1,2 +1,2 @@\n same\n-gone\n+here\n";
    x::open_generated("gentest", spec, text.into(), None, Vec::new()).unwrap();
    app.tick(Instant::now());
    let mut term = Terminal::new(TestBackend::new(60, 8)).unwrap();
    term.draw(|f| app.draw(f)).unwrap();
    let buf = term.backend().buffer().clone();
    let text_of = |y: u16| {
        (0..buf.area.width)
            .map(|x| buf[(x, y)].symbol().to_string())
            .collect::<String>()
    };
    let bg_at = |word: &str| {
        let y = (0..buf.area.height)
            .find(|&y| text_of(y).contains(word))
            .unwrap();
        let first = word.chars().next().unwrap().to_string();
        let x = (0..buf.area.width)
            .find(|&x| buf[(x, y)].symbol() == first)
            .unwrap();
        buf[(x, y)].bg
    };
    let base = bg_at("same");
    assert_ne!(bg_at("+here"), base, "an added line has a background");
    assert_ne!(bg_at("-gone"), base, "a removed line has a background");
    assert_ne!(bg_at("@@"), base, "a hunk's line has a background");
    assert_ne!(bg_at("+here"), bg_at("-gone"), "added and removed differ");
    let _ = std::fs::remove_dir_all(&dir);
}

/// A plugin's button (the git plugin's Git) stands beside the file
/// manager and the projects in the list of open files, at the top or on
/// the left, for a document whose folder is under its version control and
/// not for another; a click runs the plugin's command.
#[test]
fn a_plugins_button_shows_in_a_repository() {
    use crossterm::event::{Event, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
    use kalem_core::command::{Command, CommandHandler, CommandSource, Scope};
    use kalem_core::settings::Layer;
    use std::sync::atomic::{AtomicUsize, Ordering};
    static RAN: AtomicUsize = AtomicUsize::new(0);
    let _serial = one_at_a_time();
    let dir = std::env::temp_dir().join(format!("kalem-tui-gitbutton-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("repo/.git")).unwrap();
    std::fs::create_dir_all(dir.join("plain")).unwrap();
    for f in ["repo/a.txt", "repo/b.txt", "plain/c.txt"] {
        std::fs::write(dir.join(f), "x\n").unwrap();
    }
    x::add_command(Command {
        id: "gitbutton.status".into(),
        title: "Gitbutton: Status".into(),
        category: "Gitbutton".into(),
        default_keys: Vec::new(),
        when: None,
        handler: CommandHandler::Native(|_, _| {
            RAN.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }),
        args_schema: None,
        source: CommandSource::Plugin("gitbutton".into()),
        scope: Some(Scope::all()),
    })
    .unwrap();
    x::add_button(x::PluginButton {
        plugin: "gitbutton".into(),
        title: "Gitbutton".into(),
        command: "gitbutton.status".into(),
        when: Some(kalem_core::when::WhenClause::parse("vcs == git").unwrap()),
    });
    // Where the button's title is drawn.
    let find = |buf: &ratatui::buffer::Buffer| {
        (0..buf.area.height).find_map(|y| {
            let row: Vec<&str> = (0..buf.area.width).map(|x| buf[(x, y)].symbol()).collect();
            let row = row.concat();
            row.find("Gitbutton")
                .map(|i| (row[..i].chars().count() as u16, y))
        })
    };
    for at in ["top", "left"] {
        let config =
            Config::from_layers(&[(Layer::User, None, &format!("ui.open_files = \"{at}\"\n"))]);
        let mut app = App::with_keymap(
            Some(&dir.join("repo/a.txt")),
            config,
            Caps::full(),
            &[],
            Vec::new(),
        )
        .unwrap();
        app.open_path(&dir.join("repo/b.txt"), None);
        let mut term = ratatui::Terminal::new(ratatui::backend::TestBackend::new(80, 12)).unwrap();
        let mut draw = |app: &mut App| term.draw(|f| app.draw(f)).unwrap().buffer.clone();
        let Some((column, row)) = find(&draw(&mut app)) else {
            panic!("no button {at}");
        };
        let ran = RAN.load(Ordering::SeqCst);
        app.event(Event::Mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column,
            row,
            modifiers: KeyModifiers::NONE,
        }));
        assert_eq!(RAN.load(Ordering::SeqCst), ran + 1, "{at}");
        app.open_path(&dir.join("plain/c.txt"), None);
        assert_eq!(find(&draw(&mut app)), None, "{at}");
    }
    x::remove_buttons("gitbutton");
    x::remove_command("gitbutton.status");
    let _ = std::fs::remove_dir_all(&dir);
}
