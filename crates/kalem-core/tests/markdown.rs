//! Markdown files on disk (T2.7c.8): drawn as they read and saved with
//! every untouched byte as it was (byte order mark, CR LF, trailing
//! blanks, tabs, HTML), whatever the view hides.

use std::sync::Arc;
use std::time::Instant;

use kalem_core::{DocumentMode, DocumentState};

/// A README in the style GitHub hosts, with what an editor must not touch.
const README: &str = "\u{feff}# Project  \r\n\r\nSome *text*  \r\nwith a hard break and **bold**.\r\n\r\n- [ ] task\r\n- [x] done\r\n\r\n| a | b |\r\n|:--|--:|\r\n| 1 | 2 |\r\n\r\n```rust\r\n\tlet x = 1;\r\n```\r\n\r\n<details><summary>More</summary>\r\n\r\n![logo](logo.png)\r\n\r\n</details>\r\n\r\n[^1]: A note.\r\n";

fn open(name: &str, bytes: &str) -> (std::path::PathBuf, DocumentState) {
    let dir = std::env::temp_dir().join(format!("kalem-md-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(name);
    std::fs::write(&path, bytes).unwrap();
    let base = kalem_core::settings::Config::default().parse_base();
    let d = DocumentState::open(&path, Arc::new(org_model::Settings::default()), &base).unwrap();
    (path, d)
}

#[test]
fn untouched_bytes_stay() {
    let (path, mut d) = open("README.md", README);
    assert_eq!(d.meta.mode, DocumentMode::Markdown);
    // Every line drawn, with the cursor on it and away from it.
    let text = d.text().as_str().to_string();
    let lines = d.text().line_count();
    for l in 0..lines {
        let r = d.text().line_range(l);
        for cursor in [None, Some(r.start)] {
            let v = kalem_core::markdown::line_view(&d, r.clone(), cursor);
            // Every shown character maps back into the line.
            for run in &v.runs {
                assert!(
                    r.start <= run.src.start && run.src.end <= r.end,
                    "{run:?} in {r:?}"
                );
                if run.verbatim {
                    assert_eq!(run.text, text[run.src.clone()]);
                }
            }
        }
    }
    // Saved unedited: the same bytes.
    d.save(kalem_core::files::SaveOptions::default(), true)
        .unwrap();
    assert_eq!(std::fs::read_to_string(&path).unwrap(), README);
    // One edit through a Markdown command: only its byte changes.
    let at = text.find("[ ] task").unwrap() + 1;
    d.selection = org_edit::Selection::caret(at);
    let reg = kalem_core::CommandRegistry::with_builtins();
    let config = kalem_core::settings::Config::default();
    let mut clip = kalem_core::command::Clipboard::default();
    let mut ctx = kalem_core::command::EditorContext {
        document: Some(&mut d),
        clipboard: &mut clip,
        config: &config,
        now: Instant::now(),
        clock: jiff::civil::date(2026, 10, 1).at(9, 0, 0, 0),
        messages: Vec::new(),
        requests: Vec::new(),
    };
    reg.execute("markdown.toggleCheckbox", &mut ctx, &serde_json::json!({}))
        .unwrap();
    drop(ctx);
    d.save(kalem_core::files::SaveOptions::default(), true)
        .unwrap();
    let after = std::fs::read_to_string(&path).unwrap();
    assert_eq!(after, README.replacen("[ ] task", "[x] task", 1));
    let _ = std::fs::remove_dir_all(path.parent().unwrap());
}

#[test]
fn outline_from_the_file() {
    let (path, d) = open("notes.markdown", "Title\n=====\n\n## Part *one*\n\ntext\n");
    let items = kalem_core::markdown::outline_items(&d);
    assert_eq!(
        items
            .iter()
            .map(|i| (i.level, i.title.as_str()))
            .collect::<Vec<_>>(),
        [(1, "Title"), (2, "Part *one*")]
    );
    let _ = std::fs::remove_dir_all(path.parent().unwrap());
}
