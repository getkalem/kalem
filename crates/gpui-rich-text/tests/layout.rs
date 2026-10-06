//! The layout in gpui's headless test platform, whose text system gives
//! every glyph the same advance.

use gpui::{Hsla, Pixels, TestAppContext, TextRun, font, px, size};
use gpui_rich_text::{InlineLayout, Piece};

fn run(len: usize) -> TextRun {
    TextRun {
        len,
        font: font("Helvetica"),
        color: Hsla::black(),
        background_color: None,
        underline: None,
        strikethrough: None,
    }
}

fn text(s: &str) -> Piece {
    Piece::Text {
        text: s.into(),
        runs: vec![run(s.len())],
    }
}

fn layout(
    cx: &mut TestAppContext,
    pieces: Vec<Piece>,
    wrap: Option<Pixels>,
    hang_at: Option<usize>,
) -> InlineLayout {
    let cx = cx.add_empty_window();
    cx.update(|window, _| InlineLayout::new(&pieces, px(16.), px(23.), wrap, hang_at, window))
}

#[gpui::test]
fn one_row(cx: &mut TestAppContext) {
    let l = layout(cx, vec![text("hello world")], None, None);
    assert_eq!(l.rows.len(), 1);
    assert_eq!((l.rows[0].start, l.rows[0].end), (0, 11));
    // Carets move right; a position maps back to its offset.
    let xs: Vec<Pixels> = (0..=11).map(|i| l.caret(i).origin.x).collect();
    assert!(xs.windows(2).all(|w| w[0] < w[1]), "{xs:?}");
    for i in 0..=11 {
        let c = l.caret(i);
        assert_eq!(
            l.index_for_position(c.origin + gpui::point(px(0.5), px(2.))),
            i
        );
    }
    // A selection is one rectangle from caret to caret.
    let r = l.range_rects(2, 7);
    assert_eq!(r.len(), 1);
    assert_eq!(r[0].origin.x, xs[2]);
    assert_eq!(r[0].size.width, xs[7] - xs[2]);
}

#[gpui::test]
fn wrapping_with_a_hanging_indent(cx: &mut TestAppContext) {
    let s = "- one two three four five six seven eight";
    let full = layout(cx, vec![text(s)], None, None);
    let wrap = full.width / 2.5;
    let l = layout(cx, vec![text(s)], Some(wrap), Some(2));
    assert!(l.rows.len() >= 3, "{:?}", l.rows);
    // Rows follow each other, break after spaces and fit.
    for w in l.rows.windows(2) {
        assert_eq!(w[0].end, w[1].start);
        assert_eq!(&s[w[0].end - 1..w[0].end], " ");
        assert!(w[1].y > w[0].y);
    }
    assert_eq!(l.rows.last().unwrap().end, s.len());
    // Wrapped rows start where the text after the bullet does.
    let indent = l.caret(2).origin.x;
    for r in &l.rows[1..] {
        assert!((l.caret(r.start).origin.x - indent).abs() < px(0.01));
        assert!(l.caret(r.end).origin.x <= wrap + px(0.5), "{r:?}");
    }
    // A selection across rows: one rectangle per row.
    let (a, b) = (l.rows[0].start + 3, l.rows[1].end - 1);
    assert_eq!(l.range_rects(a, b).len(), 2);
    // A click below the last row lands in it.
    let below = gpui::point(indent, l.height + px(50.));
    assert!(l.index_for_position(below) >= l.rows.last().unwrap().start);
}

#[gpui::test]
fn widgets_and_spacers(cx: &mut TestAppContext) {
    let pieces = vec![
        text("a "),
        Piece::Widget {
            len: 3,
            size: size(px(40.), px(12.)),
            ascent: px(10.),
        },
        text(" b"),
        Piece::Spacer {
            len: 1,
            min: px(8.),
        },
        text(":tag:"),
    ];
    let l = layout(cx, pieces, Some(px(600.)), None);
    let widgets: Vec<_> = l.widgets().collect();
    assert_eq!(widgets.len(), 1);
    let (at, b) = widgets[0];
    assert_eq!(at, 2);
    assert_eq!(b.size, size(px(40.), px(12.)));
    assert_eq!(b.origin.x, l.caret(2).origin.x);
    assert_eq!(l.caret(5).origin.x, b.origin.x + px(40.));
    // The spacer pushes the tags to the right edge.
    let end = l.caret(5 + 2 + 1 + 5).origin.x;
    assert!((end - px(600.)).abs() < px(1.), "{end:?}");
    // A click on the widget's right half lands after it.
    let right = b.origin + gpui::point(px(35.), px(5.));
    assert_eq!(l.index_for_position(right), 5);
}

#[gpui::test]
fn scripts_are_raised_and_lowered(cx: &mut TestAppContext) {
    let pieces = vec![
        text("x"),
        Piece::Script {
            text: "2".into(),
            runs: vec![run(1)],
            sup: true,
        },
        text(" H"),
        Piece::Script {
            text: "2".into(),
            runs: vec![run(1)],
            sup: false,
        },
    ];
    let l = layout(cx, pieces, None, None);
    assert_eq!((l.rows.len(), l.rows[0].end), (1, 5));
    // Script glyphs are narrower than body glyphs.
    let body = l.caret(1).origin.x - l.caret(0).origin.x;
    let sup = l.caret(2).origin.x - l.caret(1).origin.x;
    assert!(sup < body, "{sup:?} {body:?}");
}

#[gpui::test]
fn justified_rows(cx: &mut TestAppContext) {
    let s = "aa bb cc dd ee ff gg hh";
    let plain = layout(cx, vec![text(s)], None, None);
    let wrap = plain.caret(12).origin.x;
    let mut l = layout(cx, vec![text(s)], Some(wrap), None);
    assert!(l.rows.len() > 1);
    let first = l.rows[0];
    let last_char_end = |l: &InlineLayout| {
        let r = l.rows[0];
        let end = s[..r.end].trim_end().len();
        l.caret(end).origin.x
    };
    let before = last_char_end(&l);
    l.justify();
    // The first row now reaches the wrap width; its first word stays put.
    let after = last_char_end(&l);
    assert!(after > before, "{after:?} {before:?}");
    assert!((after - wrap).abs() < px(0.5), "{after:?} {wrap:?}");
    assert_eq!(l.caret(0).origin.x, plain.caret(0).origin.x);
    // Carets and hit testing agree on the stretched row.
    for i in first.start..=s[..first.end].trim_end().len() {
        let c = l.caret(i);
        assert_eq!(
            l.index_for_position(c.origin + gpui::point(px(0.3), px(2.))),
            i
        );
    }
    // The last row is not stretched.
    let k = l.rows.len() - 1;
    let r = l.rows[k];
    let start_x = l.caret(r.start).origin.x;
    let end_x = l.caret(r.end).origin.x;
    assert!(end_x - start_x < wrap, "{start_x:?} {end_x:?}");
    let _ = size(px(1.), px(1.));
}

#[gpui::test]
fn blocks_wrap_inside_their_width(cx: &mut TestAppContext) {
    // A table row: a gap, a narrow cell whose text wraps, a gap, a cell
    // of one word.
    let gap = |w: f32| Piece::Widget {
        len: 3,
        size: size(px(w), px(0.)),
        ascent: px(0.),
    };
    let long = "one two three four five six";
    let pieces = vec![
        gap(4.),
        Piece::Block {
            pieces: vec![text(long)],
            width: px(80.),
        },
        gap(8.),
        Piece::Block {
            pieces: vec![text("x")],
            width: px(40.),
        },
        gap(4.),
    ];
    let l = layout(cx, pieces, Some(px(400.)), None);
    // One row of the line, as high as the wrapped cell.
    assert_eq!(l.rows.len(), 1);
    let single = layout(cx, vec![text("x")], Some(px(400.)), None).height;
    assert!(l.height > single * 2., "{} {}", l.height, single);
    // The first cell's offsets are inside its box, its later words
    // lower down; the second cell is right of the box, on the first row.
    let start = 3;
    let first = l.caret(start);
    let last = l.caret(start + long.len());
    assert!(
        first.origin.x >= px(4.) && last.origin.x < px(84.),
        "{first:?} {last:?}"
    );
    assert!(last.origin.y > first.origin.y);
    let x = start + long.len() + 3;
    let c = l.caret(x);
    assert!(
        c.origin.x >= px(92.) && c.origin.y == first.origin.y,
        "{c:?}"
    );
    // A position maps back into the cell under it.
    let i = l.index_for_position(last.origin + gpui::point(px(-0.5), px(2.)));
    assert!(i > start && i <= start + long.len(), "{i}");
    // A selection across the wrapped cell has a rectangle on each of its
    // rows.
    let r = l.range_rects(start, start + long.len());
    assert!(r.len() >= 3, "{r:?}");
}
