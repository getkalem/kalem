//! A plugin's outline (plugin API 0.2.18, the `outline` export) in the
//! outline sidebar: asked for a document of its types where neither the
//! mode nor a language pack gives one, once a version of the text; the
//! last kept while the plugin is busy, none when it fails, and none once
//! the plugin is gone.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Instant;

use kalem_core::DocumentState;
use kalem_core::outlines::{self, Answer, PluginOutlineSpec};
use kalem_core::view::OutlineItem;

static ASKED: AtomicUsize = AtomicUsize::new(0);
static MODE: AtomicUsize = AtomicUsize::new(0);

fn heading(level: usize, title: &str, start: usize) -> OutlineItem {
    OutlineItem {
        level,
        todo: None,
        title: title.into(),
        start,
        file: None,
    }
}

fn set(doc: &mut DocumentState, text: &str) {
    let mut tx = org_edit::Transaction::new("set");
    tx.replace(0..doc.text().len(), text).expect("edit");
    doc.apply(&tx, org_edit::ChangeKind::Command, Instant::now());
}

fn titles(items: Option<Vec<OutlineItem>>) -> Option<Vec<(usize, String, usize)>> {
    items.map(|v| v.into_iter().map(|i| (i.level, i.title, i.start)).collect())
}

#[test]
fn a_plugins_outline_in_the_sidebar() {
    let dir = std::env::temp_dir().join(format!("kalem-plugin-outline-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("folder");
    let file = dir.join("a.oltest");
    std::fs::write(&file, "* One\ntext\n* Two\n").expect("file");
    let mut doc = DocumentState::open(
        &file,
        Arc::new(org_model::Settings::default()),
        &org_syntax::ParseContext::default(),
    )
    .expect("open");
    assert!(kalem_core::mode_view::outline_items(&mut doc).is_none_or(|o| o.is_empty()));

    outlines::register(PluginOutlineSpec {
        plugin: "oltest".into(),
        id: "stars".into(),
        types: vec!["oltest".into()],
    });
    outlines::set_provider(Arc::new(|spec, path, text, kind| {
        assert_eq!((spec.id.as_str(), kind), ("stars", "oltest"));
        assert!(path.is_some_and(|p| p.ends_with("a.oltest")));
        ASKED.fetch_add(1, Ordering::SeqCst);
        match MODE.load(Ordering::SeqCst) {
            0 => {
                let mut at = 0;
                let mut out = Vec::new();
                for line in text.split_inclusive('\n') {
                    if let Some(t) = line.strip_prefix("* ") {
                        out.push(heading(1, t.trim(), at));
                    }
                    at += line.len();
                }
                // A start past the text is brought back into it.
                out.push(heading(2, "Past", 10_000));
                Answer::Headings(out)
            }
            1 => Answer::Busy,
            _ => Answer::Failed("broken".into()),
        }
    }));
    let len = doc.text().as_str().len();
    let want = vec![
        (1, "One".to_string(), 0),
        (1, "Two".to_string(), 11),
        (2, "Past".to_string(), len),
    ];
    assert_eq!(
        titles(kalem_core::mode_view::outline_items(&mut doc)),
        Some(want.clone())
    );
    // Once a version: asked once for two looks.
    assert_eq!(
        titles(kalem_core::mode_view::outline_items(&mut doc)),
        Some(want.clone())
    );
    assert_eq!(ASKED.load(Ordering::SeqCst), 1);
    // A new version while the plugin is busy: the last stays.
    set(&mut doc, "* One\ntext\n* Two\n* Three\n");
    MODE.store(1, Ordering::SeqCst);
    assert_eq!(
        titles(kalem_core::mode_view::outline_items(&mut doc)),
        Some(want)
    );
    // Then asked again.
    MODE.store(0, Ordering::SeqCst);
    let now = titles(kalem_core::mode_view::outline_items(&mut doc)).expect("an outline");
    assert_eq!(
        now.iter().map(|t| t.1.as_str()).collect::<Vec<_>>(),
        ["One", "Two", "Three", "Past"]
    );
    // A plugin that fails gives none.
    set(&mut doc, "\n* One\n");
    MODE.store(2, Ordering::SeqCst);
    assert!(outlines::outline_items(&doc).is_none());

    // Another type: not asked.
    let other = dir.join("b.zzzol");
    std::fs::write(&other, "* X\n").expect("file");
    let b = DocumentState::open(
        &other,
        Arc::new(org_model::Settings::default()),
        &org_syntax::ParseContext::default(),
    )
    .expect("open");
    let asked = ASKED.load(Ordering::SeqCst);
    assert!(outlines::outline_items(&b).is_none());
    assert_eq!(ASKED.load(Ordering::SeqCst), asked);
    // The plugin gone: none.
    MODE.store(0, Ordering::SeqCst);
    outlines::remove_plugin("oltest");
    assert!(outlines::outline_items(&doc).is_none());
    let _ = std::fs::remove_dir_all(dir);
}
