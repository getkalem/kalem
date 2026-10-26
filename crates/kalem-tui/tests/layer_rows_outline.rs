//! Lines a plugin's layer draws under a line that start blocks of a
//! file's outline (plugin API 0.2.18), typed into in the terminal as that
//! outline: Enter splits a block into a new one after it and the blocks
//! under it, Tab and Shift+Tab move it a step in and out with them, each
//! written into the file and typed into on where it went. In a test
//! binary of its own, as the layers are the process's.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
use kalem_core::layers::{
    self, Below, LayerSpec, LineSource, Outcome, Outline, Overlays, VirtualLine,
};
use kalem_core::settings::Config;
use kalem_core::view::Style;
use kalem_tui::app::App;
use kalem_tui::caps::Caps;

const NOTE: &str = "- first\n- embed here\n- third\n";

/// A page's blocks as an embed shows them, each the start of a block of
/// the page's outline (a tab a level).
fn rows(page: &Path) -> Vec<VirtualLine> {
    let file = std::fs::read_to_string(page).unwrap_or_default();
    let lines: Vec<&str> = file.lines().collect();
    let depth = |l: &str| l.len() - l.trim_start_matches('\t').len();
    (0..lines.len())
        .map(|i| {
            let d = depth(lines[i]);
            let end = (i + 1..lines.len())
                .find(|&j| depth(lines[j]) <= d)
                .unwrap_or(lines.len());
            let before = lines[..i].iter().rev().find(|l| depth(l) <= d);
            let text = lines[i][d + 2..].to_string();
            VirtualLine {
                indent: 2 + 2 * d,
                pieces: vec![(format!("• {text}"), Style::default())],
                source: Some(LineSource {
                    path: page.to_path_buf(),
                    line: i,
                    column: d + 2,
                    text,
                    outline: Some(Outline {
                        end,
                        next: end,
                        bullet: format!("{}- ", "\t".repeat(d)),
                        step: "\t".into(),
                        indent: before.is_some_and(|l| depth(l) == d),
                        outdent: d > 0,
                    }),
                }),
            }
        })
        .collect()
}

fn key(app: &mut App, code: KeyCode) {
    app.event(Event::Key(KeyEvent::new(code, KeyModifiers::NONE)));
}

fn typ(app: &mut App, s: &str) {
    for c in s.chars() {
        key(app, KeyCode::Char(c));
    }
}

/// What is typed into, and the text before the cursor.
fn editing(app: &App) -> Option<(String, String)> {
    let r = app.doc.plugins.row_edit.as_ref()?;
    Some((r.input.clone(), r.input[..r.cursor()].to_string()))
}

#[test]
fn blocks_split_and_moved_where_an_embed_shows_them() {
    let dir = std::env::temp_dir().join(format!("kalem-tui-rows-outline-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("notes.marker"), "").unwrap();
    let path = dir.join("n.md");
    std::fs::write(&path, NOTE).unwrap();
    let page: PathBuf = dir.join("page.md");
    std::fs::write(&page, "- one\n\t- child\n- two\n").unwrap();
    kalem_core::extensions::name_folder(dir.clone());
    layers::register(LayerSpec {
        plugin: "test".into(),
        id: "notes".into(),
        markers: vec!["notes.marker".into()],
        modes: vec!["markdown".into()],
        open: None,
    });
    let embedded = page.clone();
    layers::set_provider(Arc::new(move |_, _, text: &str| {
        Outcome::Done(Overlays {
            below: vec![Below {
                line: text
                    .find("embed")
                    .map_or(0, |e| text[..e].rfind('\n').map_or(0, |n| n + 1)),
                lines: rows(&embedded),
            }],
            ..Overlays::default()
        })
    }));
    let mut app = App::with_keymap(
        Some(&path),
        Config::default(),
        Caps::full(),
        &[],
        Vec::new(),
    )
    .unwrap();
    let page_text = || std::fs::read_to_string(&page).unwrap();
    app.doc.move_cursor(NOTE.find("embed").unwrap(), false);
    app.run_command("view.editRow", serde_json::json!({}));
    assert_eq!(editing(&app), Some(("one".into(), "one".into())));
    // Enter at the end: a new block after "one" and its child, typed
    // into at once.
    key(&mut app, KeyCode::Enter);
    assert_eq!(page_text(), "- one\n\t- child\n- \n- two\n");
    assert_eq!(editing(&app), Some((String::new(), String::new())));
    typ(&mut app, "new");
    // Down writes it and goes on to "two".
    key(&mut app, KeyCode::Down);
    assert_eq!(page_text(), "- one\n\t- child\n- new\n- two\n");
    assert_eq!(editing(&app).map(|e| e.0), Some("two".into()));
    // Tab: under "new"; Shift+Tab: back; typed into on all along.
    key(&mut app, KeyCode::Tab);
    assert_eq!(page_text(), "- one\n\t- child\n- new\n\t- two\n");
    assert_eq!(editing(&app).map(|e| e.0), Some("two".into()));
    key(&mut app, KeyCode::BackTab);
    assert_eq!(page_text(), "- one\n\t- child\n- new\n- two\n");
    // A block with nothing before it at its level stays where it is; one
    // at the top, too, for Shift+Tab.
    key(&mut app, KeyCode::BackTab);
    assert_eq!(page_text(), "- one\n\t- child\n- new\n- two\n");
    assert!(app.doc.plugins.row_edit.is_some());
    // Enter in the middle: what follows the cursor a new block, the
    // cursor at its start.
    key(&mut app, KeyCode::Home);
    key(&mut app, KeyCode::Right);
    key(&mut app, KeyCode::Enter);
    assert_eq!(page_text(), "- one\n\t- child\n- new\n- t\n- wo\n");
    assert_eq!(editing(&app), Some(("wo".into(), String::new())));
    // Escape: nothing more written.
    typ(&mut app, "x");
    key(&mut app, KeyCode::Esc);
    assert_eq!(page_text(), "- one\n\t- child\n- new\n- t\n- wo\n");
    assert_eq!(app.doc.text().as_str(), NOTE);
    let _ = std::fs::remove_dir_all(&dir);
}
