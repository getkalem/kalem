//! A plugin's document (`documents`, plugin API 0.2.5) in the terminal
//! editor: shown when the plugin opens it, written again in place,
//! never saved, and forgotten once closed. In a test binary of its own, as
//! the plugins' requests are the process's.

use kalem_core::extensions::{self as x, GeneratedSpec};
use kalem_core::settings::Config;
use kalem_tui::app::App;
use kalem_tui::caps::Caps;
use std::time::Instant;

#[test]
fn a_plugins_document_is_shown_rewritten_and_closed() {
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
