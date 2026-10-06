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

#[test]
fn front_matter_folds_away_from_the_cursor() {
    let text = "---\ntitle: Notes\ntags: [a, b]\n---\n# Heading\n\ntext\n";
    let (path, mut d) = open("fm.md", text);
    let blocks = kalem_core::markdown::blocks(&d);
    assert_eq!(blocks[0].range, 0..text.find("# Heading").unwrap());
    let folds = kalem_core::view::Folds::default();
    // The cursor in the text: the front matter shows its first line.
    d.selection = org_edit::Selection::caret(text.len() - 2);
    let v = kalem_core::view::visible(text, &blocks, &folds, d.selection.head);
    assert!(v.folded.contains(&0));
    assert_eq!(v.ranges[0], 0..4);
    // The cursor in it: all of it.
    let v = kalem_core::view::visible(text, &blocks, &folds, 6);
    assert!(v.folded.is_empty());
    assert_eq!(v.ranges, vec![0..text.len()]);
    // Without front matter, no blocks: nothing folds.
    let (p2, d2) = open("plain.md", "# A\n\n---\n\ntext\n");
    assert!(kalem_core::markdown::blocks(&d2).is_empty());
    let _ = std::fs::remove_dir_all(path.parent().unwrap());
    let _ = std::fs::remove_dir_all(p2.parent().unwrap());
}

#[test]
fn display_math_over_several_lines_is_a_math_block() {
    // `$$` on lines of their own, as in GitHub's Markdown: one formula
    // drawn on its first line away from the cursor, as Org draws a LaTeX
    // environment. In a paragraph's text, not.
    let text = "Text.\n\n$$\na^2 +\nb^2\n$$\n\nInline $$x$$ and\nmore $$y\nz$$ here.\n";
    let (path, d) = open("math.md", text);
    let blocks = kalem_core::markdown::blocks(&d);
    let kinds: Vec<_> = blocks
        .iter()
        .map(|b| (b.kind.clone(), b.range.clone()))
        .collect();
    let math = text.find("$$\na").unwrap();
    let after = text.find("b^2\n$$\n").unwrap() + "b^2\n$$\n".len();
    assert_eq!(
        kinds,
        [
            (kalem_core::view::BlockKind::Paragraph, 0..math),
            (kalem_core::view::BlockKind::Math, math..after),
            (kalem_core::view::BlockKind::Paragraph, after..text.len()),
        ]
    );
    assert_eq!(blocks[1].content_end, after - 1);
    let _ = std::fs::remove_dir_all(path.parent().unwrap());
}

#[test]
fn toml_front_matter_is_front_matter() {
    // Hugo's `+++` … `+++`: folded as YAML's is, its `# comment` no
    // heading, drawn dimmed and in the fixed font.
    let text = "+++\ntitle = \"Notes\"\n# a comment\n+++\n# Heading\n\ntext\n";
    let (path, mut d) = open("toml.md", text);
    let blocks = kalem_core::markdown::blocks(&d);
    assert_eq!(blocks[0].range, 0..text.find("# Heading").unwrap());
    let titles: Vec<String> = kalem_core::markdown::outline_items(&d)
        .into_iter()
        .map(|i| i.title)
        .collect();
    assert_eq!(titles, ["Heading"]);
    d.selection = org_edit::Selection::caret(text.len() - 2);
    let v = kalem_core::markdown::line_view(&d, d.text().line_range(2), Some(text.len() - 2));
    assert!(v.mono && v.runs.iter().all(|r| r.style.dim), "{v:?}");
    // As HTML, no front matter in the page.
    let html = kalem_core::markdown::to_html(text);
    assert!(
        !html.contains("title") && html.contains("<h1>Heading</h1>"),
        "{html}"
    );
    let _ = std::fs::remove_dir_all(path.parent().unwrap());
}

/// Real READMEs and a vault of notes with wiki links (`tests/corpus/
/// markdown`, T2.7c.8): every line drawn with the cursor on it and away,
/// every shown character from its line, every heading in the outline, and
/// each file saved unedited byte for byte.
#[test]
fn the_markdown_corpus_reads_and_saves_as_it_was() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/corpus/markdown");
    let mut files = Vec::new();
    let mut stack = vec![root.clone()];
    while let Some(d) = stack.pop() {
        for e in std::fs::read_dir(&d).unwrap().flatten() {
            let p = e.path();
            if p.is_dir() {
                stack.push(p);
            } else if p.extension().is_some_and(|x| x == "md") {
                files.push(p);
            }
        }
    }
    files.sort();
    assert!(files.len() > 90, "{}", files.len());
    let mut wiki = 0;
    for f in files {
        let bytes = std::fs::read_to_string(&f).unwrap();
        let name = f
            .strip_prefix(&root)
            .unwrap()
            .to_string_lossy()
            .replace(['/', '\\'], "_");
        let (path, mut d) = open(&name, &bytes);
        assert_eq!(d.meta.mode, DocumentMode::Markdown, "{name}");
        let text = d.text().as_str().to_string();
        for l in 0..d.text().line_count() {
            let r = d.text().line_range(l);
            for cursor in [None, Some(r.start)] {
                let v = kalem_core::markdown::line_view(&d, r.clone(), cursor);
                for run in &v.runs {
                    assert!(
                        r.start <= run.src.start && run.src.end <= r.end,
                        "{name} line {l}: {run:?} in {r:?}"
                    );
                    if run.verbatim {
                        assert_eq!(run.text, text[run.src.clone()], "{name} line {l}");
                    }
                }
            }
        }
        // The outline has a heading for each ATX heading line outside code.
        let outline = kalem_core::markdown::outline_items(&d);
        if text.lines().any(|l| l.starts_with("# ")) {
            assert!(!outline.is_empty(), "{name}");
        }
        let md = kalem_core::markdown::Md::parse(&text);
        wiki += md
            .nodes
            .iter()
            .filter(|n| matches!(n.kind, kalem_core::markdown::MdKind::WikiLink { .. }))
            .count();
        d.save(kalem_core::files::SaveOptions::default(), true)
            .unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), bytes, "{name}");
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }
    // The vault's notes link to each other.
    assert!(wiki > 100, "{wiki} wiki links");
}
