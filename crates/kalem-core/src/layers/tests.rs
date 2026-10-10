use super::*;
use crate::DocumentMode;
use crate::document::DocumentState;
use crate::view;

fn shown(v: &LineView) -> String {
    v.runs.iter().map(|r| r.text.as_str()).collect()
}

fn verbatim(text: &str, range: Range<usize>) -> LineView {
    view::plain_line_view(text, range, None)
}

fn tag() -> Style {
    Style {
        tag: true,
        ..Style::default()
    }
}

#[test]
fn spans_hide_replace_and_style_away_from_the_cursor() {
    let text = "- see ((6512)) and #tag id:: x\n";
    let line = 0..text.len() - 1;
    let o = Overlays {
        spans: vec![
            Span {
                range: 6..14,
                effect: SpanEffect::Replace {
                    text: "the block".into(),
                    style: Style {
                        link: true,
                        ..Style::default()
                    },
                },
            },
            Span {
                range: 19..23,
                effect: SpanEffect::Style(tag()),
            },
            Span {
                range: 23..30,
                effect: SpanEffect::Hide,
            },
        ],
        lines: Vec::new(),
    }
    .normalized(text);
    let mut v = verbatim(text, line.clone());
    apply_line(&mut v, &o, None);
    assert_eq!(shown(&v), "- see the block and #tag");
    let link = v.runs.iter().find(|r| r.text == "the block").unwrap();
    assert!(link.style.link && !link.verbatim && link.src == (6..14));
    assert!(v.runs.iter().find(|r| r.text == "#tag").unwrap().style.tag);
    // The source offsets still map: the end of the line is the end.
    assert_eq!(v.display_offset(line.end), shown(&v).len());
    // The cursor in a span shows it as source; a style stays.
    let mut v = verbatim(text, line.clone());
    apply_line(&mut v, &o, Some(8));
    assert_eq!(shown(&v), "- see ((6512)) and #tag");
    assert!(v.runs.iter().find(|r| r.text == "#tag").unwrap().style.tag);
}

#[test]
fn a_span_over_a_replaced_run_is_left_out() {
    // A run the mode replaced (a wiki link showing its title) cannot be
    // split: a span cutting it is dropped, one around it applies.
    let mut v = LineView {
        range: 0..9,
        runs: vec![
            Run {
                src: 0..2,
                text: String::new(),
                verbatim: false,
                style: Style::default(),
                widget: None,
            },
            Run {
                src: 2..7,
                text: "Kalem".into(),
                verbatim: false,
                style: Style::default(),
                widget: None,
            },
            Run {
                src: 7..9,
                text: String::new(),
                verbatim: false,
                style: Style::default(),
                widget: None,
            },
        ],
        ..LineView::default()
    };
    let cut = Overlays {
        spans: vec![Span {
            range: 3..5,
            effect: SpanEffect::Hide,
        }],
        lines: Vec::new(),
    };
    apply_line(&mut v, &cut, None);
    assert_eq!(shown(&v), "Kalem");
    let around = Overlays {
        spans: vec![Span {
            range: 0..9,
            effect: SpanEffect::Style(tag()),
        }],
        lines: Vec::new(),
    };
    apply_line(&mut v, &around, None);
    assert!(v.runs[1].style.tag);
}

#[test]
fn overlays_are_normalized() {
    let text = "ab\ncd\nef";
    let o = Overlays {
        spans: vec![
            Span {
                range: 4..9,
                effect: SpanEffect::Hide,
            },
            Span {
                range: 1..2,
                effect: SpanEffect::Hide,
            },
            Span {
                range: 0..2,
                effect: SpanEffect::Hide,
            },
            Span {
                range: 2..2,
                effect: SpanEffect::Hide,
            },
        ],
        lines: vec![
            Lines {
                range: 4..5,
                effect: LineEffect::Hidden,
            },
            Lines {
                range: 3..4,
                effect: LineEffect::Folded,
            },
        ],
    }
    .normalized(text);
    // Outside the text, empty, or overlapping an earlier one: dropped.
    assert_eq!(
        o.spans.iter().map(|s| s.range.clone()).collect::<Vec<_>>(),
        vec![0..2]
    );
    // Lines widened to whole lines; of two over the same lines, the one
    // given first kept.
    assert_eq!(
        o.lines,
        [Lines {
            range: 3..6,
            effect: LineEffect::Hidden
        }]
    );
    // A multi-byte character is never cut.
    let o = Overlays {
        spans: vec![Span {
            range: 1..2,
            effect: SpanEffect::Hide,
        }],
        lines: Vec::new(),
    }
    .normalized("çay");
    assert!(o.spans.is_empty());
}

#[test]
fn hidden_lines_are_a_block_of_their_own() {
    let text = "- one\n  id:: 1\n  collapsed:: true\n- two\n";
    let b = |kind: BlockKind, range: Range<usize>| Block {
        kind,
        content_end: range.end,
        range,
        depth: 0,
        headline: None,
    };
    let blocks = vec![
        b(BlockKind::ListItem, 0..34),
        b(BlockKind::ListItem, 34..40),
    ];
    let o = Overlays {
        spans: Vec::new(),
        lines: vec![Lines {
            range: 6..34,
            effect: LineEffect::Hidden,
        }],
    }
    .normalized(text);
    let cut = apply_blocks(blocks, &o);
    assert_eq!(
        cut.iter()
            .map(|b| (b.kind.clone(), b.range.clone()))
            .collect::<Vec<_>>(),
        [
            (BlockKind::ListItem, 0..6),
            (BlockKind::Hidden, 6..34),
            (BlockKind::ListItem, 34..40)
        ]
    );
    let folds = view::Folds::default();
    let away = view::visible(text, &cut, &folds, 36);
    assert_eq!(away.ranges, [0..6, 34..40]);
    let on = view::visible(text, &cut, &folds, 10);
    assert_eq!(on.ranges, vec![0..40]);
}

/// A layer written in Rust over a Markdown document: the registry, the
/// marker, the provider and the document's cache.
#[test]
fn a_layer_over_markdown() {
    let dir = std::env::temp_dir().join(format!("kalem-layers-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("graph/logseq")).unwrap();
    std::fs::create_dir_all(dir.join("graph/pages")).unwrap();
    std::fs::write(dir.join("graph/logseq/config.edn"), "{}").unwrap();
    let asked = Arc::new(AtomicU64::new(0));
    let count = asked.clone();
    set_provider(Arc::new(
        move |spec: &LayerSpec, _: Option<&Path>, text: &str| {
            count.fetch_add(1, Ordering::Relaxed);
            assert_eq!(spec.id, "test.ids");
            let mut o = Overlays::default();
            let mut at = 0;
            for line in text.split_inclusive('\n') {
                if line.trim_start().starts_with("id::") {
                    o.lines.push(Lines {
                        range: at..at + line.len(),
                        effect: LineEffect::Hidden,
                    });
                }
                at += line.len();
            }
            Outcome::Done(o)
        },
    ));
    register(LayerSpec {
        plugin: "org.test.layer".into(),
        id: "test.ids".into(),
        markers: vec!["logseq/config.edn".into()],
        modes: vec!["markdown".into()],
    });
    let text = "- one **bold**\n  id:: 6512\n- two\n";
    let doc = |path: std::path::PathBuf| {
        let meta = crate::Metadata {
            path: Some(path.clone()),
            mode: DocumentMode::detect(Some(&path), text.as_bytes()),
            line_ending: crate::LineEnding::Lf,
            bom: false,
            encoding: encoding_rs::UTF_8,
            lossy: false,
        };
        DocumentState::new(text, meta, Arc::new(org_model::Settings::default()))
    };
    let d = doc(dir.join("graph/pages/a.md"));
    let blocks = crate::mode_view::blocks(&d);
    assert!(
        blocks
            .iter()
            .any(|b| b.kind == BlockKind::Hidden && b.range == (15..27)),
        "{blocks:?}"
    );
    // The mode's own drawing stays: bold without its markers.
    let v = crate::mode_view::line_view(&d, false, d.text().line_range(0), None, None);
    assert_eq!(shown(&v), "- one bold");
    // Asked once for the version.
    let n = asked.load(Ordering::Relaxed);
    let _ = crate::mode_view::blocks(&d);
    assert_eq!(asked.load(Ordering::Relaxed), n);
    // A file outside the graph, or in another mode: no layer.
    let outside = doc(dir.join("a.md"));
    assert!(outside.overlays().is_none());
    assert!(
        !crate::mode_view::blocks(&outside)
            .iter()
            .any(|b| b.kind == BlockKind::Hidden)
    );
    // The source view shows the text as it is.
    let v = crate::mode_view::line_view(&d, true, d.text().line_range(1), None, None);
    assert_eq!(shown(&v), "  id:: 6512");
    // The when-clause key naming the layer, for keys of its documents.
    let key = |c: &crate::when::Context| match c.get("editorLayer") {
        Some(crate::when::Value::Str(s)) => Some(s.clone()),
        _ => None,
    };
    assert_eq!(
        key(&d.when_context()).as_deref(),
        Some("org.test.layer.test.ids")
    );
    assert_eq!(
        key(&d.document_context()).as_deref(),
        Some("org.test.layer.test.ids")
    );
    assert_eq!(key(&outside.when_context()), None);
    remove_plugin("org.test.layer");
    assert!(d.overlays().is_none());
    let _ = std::fs::remove_dir_all(&dir);
}
