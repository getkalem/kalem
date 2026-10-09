//! The `flow-viewer` world end to end: the fake document of
//! `tests/plugins/flowdoc`, a viewer of the Rust contract exported with
//! the `flow` and `annotations` interfaces, opened through the host as the
//! contract again, so every value crosses the boundary both ways (API
//! 0.2.7). Skipped where the `wasm32-unknown-unknown` target or
//! `wasm-tools` is not installed.

use std::sync::Arc;

mod common;

use kalem_script::Host;
use kalem_script::viewer::{ComponentViewer, VIEWER_LIMITS};
use kalem_viewer::{
    Anchor, AnnotationKind, AsideKind, FlowItem, FlowPlace, FlowRole, Piece, Script, Viewer,
};

fn open(text: &str) -> Option<Box<dyn kalem_viewer::ViewerDocument>> {
    let bytes = common::component("flowdoc")?;
    let dir =
        std::env::temp_dir().join(format!("kalem-flow-{}-{}", std::process::id(), text.len()));
    std::fs::create_dir_all(&dir).unwrap();
    let wasm = dir.join("flowdoc.wasm");
    std::fs::write(&wasm, bytes).unwrap();
    let file = dir.join("a.flow");
    std::fs::write(&file, text).unwrap();
    let host = Arc::new(Host::new(None).unwrap());
    let v = ComponentViewer::new(
        host,
        &wasm,
        "flowdoc",
        "Flow document",
        &["flow".into()],
        VIEWER_LIMITS,
    );
    Some(v.open(kalem_viewer::FileHandle::new(&file)).unwrap())
}

const DOC: &str = "# Title\nPlain and *bold* text^\n- an item\n|a|b|\n---\nThe note.\n";

#[test]
fn a_flow_crosses_to_the_host() {
    let Some(mut d) = open(DOC) else {
        return;
    };
    let l = d.flow(0).expect("a flow");
    assert!(l.editable);
    assert_eq!(d.flow(1), None);
    let items = d.flow_items(0, 0, l.items);
    assert_eq!(items.len() as u32, l.items);
    let FlowItem::Paragraph(h) = &items[0] else {
        panic!("{:?}", items[0])
    };
    assert_eq!(
        (h.role, h.level, h.style.as_str()),
        (FlowRole::Heading, 1, "Heading 1")
    );
    assert_eq!(h.index, Some(0));
    let FlowItem::Paragraph(p) = &items[1] else {
        panic!()
    };
    assert_eq!(p.text, "Plain and bold text\u{FFFC}");
    assert_eq!(p.runs.len(), 4);
    assert!(p.runs[1].marks.bold && !p.runs[0].marks.bold);
    assert_eq!(p.runs[1].source, 10..14);
    assert_eq!(p.runs[3].piece, Piece::NoteMark("n1".into()));
    assert_eq!(p.runs[3].marks.script, Script::Superscript);
    assert!(p.runs[3].locked.is_some());
    let FlowItem::Paragraph(item) = &items[2] else {
        panic!()
    };
    assert_eq!(item.label.as_ref().map(|l| l.0.as_str()), Some("•"));
    // A table of one row of two cells, a rule, the note in an aside.
    assert!(matches!(items[3], FlowItem::TableStart(ref t) if t.columns == [72.0, 72.0]));
    assert!(matches!(items[4], FlowItem::RowStart(_)));
    assert!(matches!(items[5], FlowItem::CellStart(ref c) if c.columns == 1));
    assert!(matches!(items[6], FlowItem::Paragraph(ref c) if c.text == "a" && c.index.is_none()));
    assert!(matches!(items[7], FlowItem::CellEnd));
    assert!(matches!(items[11], FlowItem::RowEnd));
    assert!(matches!(items[12], FlowItem::TableEnd));
    assert_eq!(items[13], FlowItem::Rule("line".into()));
    assert!(
        matches!(items[14], FlowItem::AsideStart(ref a) if a.kind == AsideKind::Footnote && a.id == "n1")
    );
    assert!(matches!(items[16], FlowItem::AsideEnd));
    // A range at a time.
    assert_eq!(d.flow_items(0, 1, 2), items[1..3]);
    assert!(d.flow_items(0, 100, 5).is_empty());
    assert_eq!(d.flow_styles()[0].name, "Normal");
    assert!(d.flow_picture(0, "none", 64).is_err());
}

#[test]
fn a_flow_is_edited_and_undone_through_the_host() {
    let Some(mut d) = open(DOC) else {
        return;
    };
    let v0 = d.flow(0).unwrap().version;
    d.flow_replace(0, 1, 0..5, "Simple").unwrap();
    assert!(d.flow(0).unwrap().version > v0);
    assert!(d.modified());
    let para = |d: &mut Box<dyn kalem_viewer::ViewerDocument>, i: usize| match &d
        .flow_items(0, i as u32, 1)[0]
    {
        FlowItem::Paragraph(p) => p.text.clone(),
        other => panic!("{other:?}"),
    };
    assert!(para(&mut d, 1).starts_with("Simple and "));
    // "Simple and bold text" and the note's mark, three bytes.
    let e = d.flow_replace(0, 1, 20..23, "").unwrap_err();
    assert!(e.0.contains("note"), "{e:?}");
    d.flow_split(
        0,
        FlowPlace {
            paragraph: 0,
            offset: 2,
        },
    )
    .unwrap();
    assert_eq!(para(&mut d, 0), "Ti");
    assert_eq!(para(&mut d, 1), "tle");
    d.flow_join(0, 0).unwrap();
    assert_eq!(para(&mut d, 0), "Title");
    d.flow_delete(
        0,
        FlowPlace {
            paragraph: 0,
            offset: 2,
        },
        FlowPlace {
            paragraph: 1,
            offset: 7,
        },
    )
    .unwrap();
    assert_eq!(para(&mut d, 0), "Tiand bold text\u{FFFC}");
    assert!(d.has_history());
    for _ in 0..4 {
        assert!(d.undo().unwrap());
    }
    assert!(d.redo().unwrap());
    assert!(d.undo().unwrap());
    assert!(!d.modified());
    assert!(
        d.flow_set_marks(0, FlowPlace::default(), FlowPlace::default(), &[])
            .is_err()
    );
    let out = d.save().unwrap();
    assert_eq!(String::from_utf8(out.bytes).unwrap(), DOC);
}

#[test]
fn annotations_cross_to_the_host() {
    let Some(mut d) = open(DOC) else {
        return;
    };
    assert!(d.annotations(None).is_empty());
    d.set_author("Ayşe");
    let on = Anchor::Flow {
        unit: 0,
        from: FlowPlace {
            paragraph: 1,
            offset: 0,
        },
        to: FlowPlace {
            paragraph: 1,
            offset: 5,
        },
    };
    let id = d.comment(on.clone(), "Check this").unwrap();
    let reply = d.reply(&id, "Done").unwrap();
    d.resolve(&id, true).unwrap();
    let all = d.annotations(Some(0));
    assert_eq!(all.len(), 2);
    assert_eq!(all[0].kind, AnnotationKind::Comment);
    assert_eq!(
        (all[0].author.as_str(), all[0].text.as_str()),
        ("Ayşe", "Check this")
    );
    assert_eq!(all[0].anchors, [on]);
    assert!(all[0].resolved);
    assert_eq!(all[1].parent.as_deref(), Some(id.as_str()));
    // The runs name the comments they are in.
    let FlowItem::Paragraph(p) = &d.flow_items(0, 1, 1)[0] else {
        panic!()
    };
    assert!(p.runs[0].annotations.contains(&id));
    assert!(d.reply("nope", "x").is_err());
    assert!(d.accept(&id).is_err());
    assert_eq!(d.tracking(), Some(false));
    d.set_tracking(true).unwrap();
    assert_eq!(d.tracking(), Some(true));
    d.remove_comment(&id).unwrap();
    assert!(
        d.annotations(None)
            .iter()
            .all(|a| a.id != id && a.id != reply)
    );
    // The edits of comments undo as the text's do.
    assert!(d.undo().unwrap());
    assert_eq!(d.annotations(None).len(), 2);
}

#[test]
fn a_viewer_without_a_flow_answers_none() {
    // The sheet exports `grid` and `annotations`, no `flow`.
    let Some(bytes) = common::component("sheet") else {
        return;
    };
    let dir = std::env::temp_dir().join(format!("kalem-noflow-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let wasm = dir.join("sheet.wasm");
    std::fs::write(&wasm, bytes).unwrap();
    let book = dir.join("a.sheet");
    std::fs::write(&book, "A\t1\n").unwrap();
    let host = Arc::new(Host::new(None).unwrap());
    let v = ComponentViewer::new(
        host,
        &wasm,
        "sheet",
        "Sheet",
        &["sheet".into()],
        VIEWER_LIMITS,
    );
    let mut d = v.open(kalem_viewer::FileHandle::new(&book)).unwrap();
    assert_eq!(d.flow(0), None);
    assert!(d.flow_items(0, 0, 10).is_empty());
    assert!(d.flow_replace(0, 0, 0..0, "x").is_err());
    assert!(d.annotations(None).is_empty());
    assert_eq!(d.tracking(), None);
    // The grid's history is still the grid's.
    assert!(d.grid(0).is_some());
}
